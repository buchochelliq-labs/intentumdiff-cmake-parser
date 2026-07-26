//! CMake parser plugin — full-parse mode on tree-sitter-cmake (issue #48). Review
//! identity: commands are labeled `identifier(first-arg)` — `add_executable(app …)`
//! keeps the target name in the identity so editing its sources pairs under the
//! target; function/macro definitions are labeled by their defined name.

use intentdiff_plugin_sdk::{
    cst::CstNode,
    ts_convert::{convert_semantic, node_to_cst},
    tree::SemanticNodeBuilder,
};

wit_bindgen::generate!({
    path: "wit/plugin.wit",
    world: "parser-plugin",
});

use crate::exports::intentdiff::plugin::parser::ExamplePair;
use crate::exports::intentdiff::plugin::parser::Guest;
use crate::exports::intentdiff::plugin::parser::LanguageInfoRecord;
use crate::exports::intentdiff::plugin::parser::ParserMode;

const LANGUAGE_ID: &str = "cmake";
const PLUGIN_METADATA: &str = include_str!("../plugin_metadata.info");

const DEFAULT_OLD: &str =
    "project(demo)\n\nadd_executable(app main.c)\n\ntarget_link_libraries(app m)\n";
const DEFAULT_NEW: &str =
    "project(demo)\n\nadd_executable(app main.c utils.c)\n\ntarget_link_libraries(app m)\n";

// Commands, definitions and arguments carry review meaning; parens, keywords and
// comments are dropped (not listed, no semantic children).
const SEMANTIC_TYPES: &[&str] = &[
    "source_file",
    "normal_command",
    "function_def",
    "macro_def",
    "block_def",
    "if_condition",
    "foreach_loop",
    "while_loop",
    "argument_list",
    "argument",
];

fn is_semantic(node_type: &str) -> bool {
    SEMANTIC_TYPES.contains(&node_type)
}

fn language_info_for(ids: Vec<String>) -> Vec<LanguageInfoRecord> {
    let metadata = intentdiff_plugin_sdk::metadata::parse_plugin_metadata(PLUGIN_METADATA);
    ids.into_iter()
        .map(|language_id| {
            let info = metadata.language_or_default(&language_id);
            LanguageInfoRecord {
                language_id: info.language_id,
                language_name: info.language_name,
                language_short_name: info.language_short_name,
                monaco_language: info.monaco_language,
                default_filename: info.default_filename,
                language_file_extensions: info.language_file_extensions,
                author: metadata.author().to_string(),
                plugin_version: metadata.plugin_version().to_string(),
                last_updated: metadata.last_updated().to_string(),
            }
        })
        .collect()
}

fn basename(path: &str) -> &str {
    path.rsplit(|ch| ch == '/' || ch == '\\')
        .next()
        .unwrap_or(path)
}

fn detect_language_impl(filename: &str, _content: &str) -> String {
    let name = basename(filename).to_lowercase();
    if name == "cmakelists.txt" || name.ends_with(".cmake") {
        LANGUAGE_ID.to_string()
    } else {
        String::new()
    }
}

/// First non-empty LEAF text under `node` (CstNode only carries text on leaves).
fn leaf_text(node: &CstNode) -> Option<String> {
    if node.is_leaf() {
        let text = node.text_or_empty().trim();
        if !text.is_empty() {
            return Some(text.chars().take(120).collect());
        }
        return None;
    }
    node.children.iter().find_map(leaf_text)
}

/// A command's review identity: `identifier(first-arg)` when a first argument exists —
/// the target/variable name is part of what the command IS about.
fn command_identity(node: &CstNode) -> Option<String> {
    let mut identifier = None;
    let mut first_arg = None;
    for child in &node.children {
        if child.node_type == "identifier" && identifier.is_none() {
            identifier = leaf_text(child);
        }
        if child.node_type == "argument_list" && first_arg.is_none() {
            first_arg = child
                .children
                .iter()
                .find(|c| c.node_type == "argument")
                .and_then(leaf_text);
        }
    }
    let identifier = identifier?;
    Some(match first_arg {
        Some(arg) => format!("{identifier}({arg})"),
        None => identifier,
    })
}

/// A definition's name: the first argument of its opening command.
fn definition_name(node: &CstNode, opener: &str) -> Option<String> {
    for child in &node.children {
        if child.node_type == opener {
            for grandchild in &child.children {
                if grandchild.node_type == "argument_list" {
                    return grandchild
                        .children
                        .iter()
                        .find(|c| c.node_type == "argument")
                        .and_then(leaf_text);
                }
            }
        }
    }
    None
}

fn label_for(node: &CstNode) -> String {
    if node.is_leaf() {
        return node.text_or_empty().trim().chars().take(120).collect();
    }
    match node.node_type.as_str() {
        "normal_command" => command_identity(node).unwrap_or_else(|| node.node_type.clone()),
        "function_def" => definition_name(node, "function_command")
            .unwrap_or_else(|| node.node_type.clone()),
        "macro_def" => {
            definition_name(node, "macro_command").unwrap_or_else(|| node.node_type.clone())
        }
        "block_def" => {
            definition_name(node, "block_command").unwrap_or_else(|| node.node_type.clone())
        }
        "argument" => leaf_text(node).unwrap_or_else(|| node.node_type.clone()),
        _ => node.node_type.clone(),
    }
}

fn parse_source(source: &str) -> Result<CstNode, String> {
    let mut parser = tree_sitter::Parser::new();
    let lang = tree_sitter_cmake::LANGUAGE.into();
    parser
        .set_language(&lang)
        .map_err(|_| "Failed to load cmake grammar".to_string())?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "tree-sitter failed to parse CMake".to_string())?;
    Ok(node_to_cst(tree.root_node(), source.as_bytes()))
}

fn process_impl(source: &str) -> String {
    let cst = match parse_source(source) {
        Ok(cst) => cst,
        Err(err) => return format!(r#"{{"error":"{}"}}"#, err),
    };
    let mut memo = std::collections::HashMap::new();
    let node = convert_semantic(&cst, "0", &mut memo, &is_semantic, &label_for).unwrap_or_else(|| {
        SemanticNodeBuilder::new("0", "source_file", LANGUAGE_ID, 0, 0, 0, 0, "0").build()
    });
    match serde_json::to_string(&node) {
        Ok(serialized) => serialized,
        Err(err) => format!(r#"{{"error":"Serialisation error: {}"}}"#, err),
    }
}

struct CmakeParser;

impl Guest for CmakeParser {
    fn get_parser_mode() -> ParserMode {
        ParserMode::FullParse
    }

    fn grammar_id() -> String {
        LANGUAGE_ID.to_string()
    }

    fn detect_language(filename: String, content: String) -> String {
        detect_language_impl(&filename, &content)
    }

    fn preprocess_source(source: String) -> String {
        source
    }

    fn example(_language: String) -> ExamplePair {
        ExamplePair {
            old: DEFAULT_OLD.to_string(),
            new: DEFAULT_NEW.to_string(),
        }
    }

    fn process(input: String, _language: String, _filename: String) -> String {
        process_impl(&input)
    }

    fn trivia_node_types() -> Vec<String> {
        vec![]
    }

    fn language_ids() -> Vec<String> {
        vec![LANGUAGE_ID.to_string()]
    }

    fn language_info() -> Vec<LanguageInfoRecord> {
        language_info_for(Self::language_ids())
    }

    fn priority() -> i32 {
        5
    }
}

export!(CmakeParser);

#[cfg(test)]
mod tests {
    use super::*;
    use intentdiff_plugin_sdk::tree::SemanticNode;

    fn labels_by_type(node: &SemanticNode, node_type: &str, out: &mut Vec<String>) {
        if node.node_type == node_type {
            out.push(node.label.clone());
        }
        for child in &node.children {
            labels_by_type(child, node_type, out);
        }
    }

    #[test]
    fn parser_mode_is_full_parse() {
        assert_eq!(CmakeParser::get_parser_mode(), ParserMode::FullParse);
    }

    #[test]
    fn detects_cmake_files() {
        assert_eq!(detect_language_impl("CMakeLists.txt", ""), LANGUAGE_ID);
        assert_eq!(detect_language_impl("cmake/deps.cmake", ""), LANGUAGE_ID);
        assert_eq!(detect_language_impl("main.rs", ""), "");
    }

    #[test]
    fn commands_carry_identifier_and_target_identity() {
        let parsed = process_impl(DEFAULT_NEW);
        intentdiff_plugin_sdk::testing::assert_valid_json(&parsed, LANGUAGE_ID);
        let root: SemanticNode = serde_json::from_str(&parsed).unwrap();
        let mut commands = Vec::new();
        labels_by_type(&root, "normal_command", &mut commands);
        assert_eq!(
            commands,
            vec![
                "project(demo)".to_string(),
                "add_executable(app)".to_string(),
                "target_link_libraries(app)".to_string(),
            ],
            "commands: {commands:?}"
        );
    }

    #[test]
    fn source_list_edit_changes_the_root_hash() {
        let old: SemanticNode = serde_json::from_str(&process_impl(DEFAULT_OLD)).unwrap();
        let new: SemanticNode = serde_json::from_str(&process_impl(DEFAULT_NEW)).unwrap();
        assert_ne!(old.structural_hash, new.structural_hash);
    }
}
