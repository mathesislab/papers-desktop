//! The nodes of the parsed LaTeX (`LatexNode` and its subclasses).

use std::sync::Arc;

use super::ParsingState;
use crate::macrospec::ParsedMacroArgs;

/// The kind of a node, with its own fields (the subclasses of `LatexNode`).
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// `LatexCharsNode`
    Chars { chars: String },
    /// `LatexGroupNode`: `{...}` (or `[...]`, for an optional argument)
    Group { nodelist: Vec<LatexNode>, delimiters: (String, String) },
    /// `LatexCommentNode`
    Comment { comment: String, comment_post_space: String },
    /// `LatexMacroNode`. `nodeargd` is `None` if the macro was read as an expression (the
    /// argument of another macro), or its arguments could not be parsed (tolerant parsing).
    Macro {
        macroname: String,
        nodeargd: Option<ParsedMacroArgs>,
        macro_post_space: String,
        /// the node was read by `get_latex_expression()` (as the argument of another macro):
        /// then the legacy `nodeoptarg` and `nodeargs` are `None`
        bare_expression: bool,
    },
    /// `LatexEnvironmentNode`
    Environment { environmentname: String, nodelist: Vec<LatexNode>, nodeargd: Option<ParsedMacroArgs> },
    /// `LatexSpecialsNode`
    Specials { specials_chars: String, nodeargd: Option<ParsedMacroArgs> },
    /// `LatexMathNode`: `displaytype` is `"inline"` or `"display"`
    Math { displaytype: String, nodelist: Vec<LatexNode>, delimiters: (String, String) },
}

/// The node classes of pylatexenc (`LatexNode.nodeType()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Chars,
    Group,
    Comment,
    Macro,
    Environment,
    Specials,
    Math,
}

impl NodeType {
    /// The Python class name, e.g. `LatexCharsNode`.
    pub fn class_name(self) -> &'static str {
        match self {
            NodeType::Chars => "LatexCharsNode",
            NodeType::Group => "LatexGroupNode",
            NodeType::Comment => "LatexCommentNode",
            NodeType::Macro => "LatexMacroNode",
            NodeType::Environment => "LatexEnvironmentNode",
            NodeType::Specials => "LatexSpecialsNode",
            NodeType::Math => "LatexMathNode",
        }
    }
}

/// A node of the parsed LaTeX. `pos` and `len` are in chars (code points) of the text of
/// `parsing_state.s`, as in pylatexenc.
///
/// Two nodes are equal if they have the same kind, fields, `pos` and `len` (the parsing state is
/// not compared).
#[derive(Debug, Clone)]
pub struct LatexNode {
    pub pos: usize,
    pub len: usize,
    pub parsing_state: Arc<ParsingState>,
    pub kind: NodeKind,
}

impl PartialEq for LatexNode {
    fn eq(&self, other: &Self) -> bool {
        self.pos == other.pos && self.len == other.len && self.kind == other.kind
    }
}

impl LatexNode {
    pub fn new(kind: NodeKind, parsing_state: Arc<ParsingState>, pos: usize, len: usize) -> Self {
        LatexNode { pos, len, parsing_state, kind }
    }

    /// `nodeType()`
    pub fn node_type(&self) -> NodeType {
        match self.kind {
            NodeKind::Chars { .. } => NodeType::Chars,
            NodeKind::Group { .. } => NodeType::Group,
            NodeKind::Comment { .. } => NodeType::Comment,
            NodeKind::Macro { .. } => NodeType::Macro,
            NodeKind::Environment { .. } => NodeType::Environment,
            NodeKind::Specials { .. } => NodeType::Specials,
            NodeKind::Math { .. } => NodeType::Math,
        }
    }

    /// `isNodeType(t)`
    pub fn is_node_type(&self, t: NodeType) -> bool {
        self.node_type() == t
    }

    /// `latex_verbatim()`: the text of the node as it was in the source.
    pub fn latex_verbatim(&self) -> String {
        self.parsing_state.s.slice(self.pos, self.len)
    }

    /// `chars` of a chars node.
    pub fn chars(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Chars { chars } => Some(chars),
            _ => None,
        }
    }

    /// `macroname` of a macro node.
    pub fn macroname(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Macro { macroname, .. } => Some(macroname),
            _ => None,
        }
    }

    /// `macro_post_space` of a macro node.
    pub fn macro_post_space(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Macro { macro_post_space, .. } => Some(macro_post_space),
            _ => None,
        }
    }

    /// `environmentname` of an environment node.
    pub fn environmentname(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Environment { environmentname, .. } => Some(environmentname),
            _ => None,
        }
    }

    /// `specials_chars` of a specials node.
    pub fn specials_chars(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Specials { specials_chars, .. } => Some(specials_chars),
            _ => None,
        }
    }

    /// `comment` of a comment node.
    pub fn comment(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Comment { comment, .. } => Some(comment),
            _ => None,
        }
    }

    /// `comment_post_space` of a comment node.
    pub fn comment_post_space(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Comment { comment_post_space, .. } => Some(comment_post_space),
            _ => None,
        }
    }

    /// `displaytype` of a math node.
    pub fn displaytype(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Math { displaytype, .. } => Some(displaytype),
            _ => None,
        }
    }

    /// `delimiters` of a group or math node.
    pub fn delimiters(&self) -> Option<(&str, &str)> {
        match &self.kind {
            NodeKind::Group { delimiters, .. } | NodeKind::Math { delimiters, .. } => {
                Some((&delimiters.0, &delimiters.1))
            }
            _ => None,
        }
    }

    /// `nodelist` of a group, environment or math node.
    pub fn nodelist(&self) -> Option<&[LatexNode]> {
        match &self.kind {
            NodeKind::Group { nodelist, .. }
            | NodeKind::Environment { nodelist, .. }
            | NodeKind::Math { nodelist, .. } => Some(nodelist),
            _ => None,
        }
    }

    /// `nodeargd` of a macro, environment or specials node.
    pub fn nodeargd(&self) -> Option<&ParsedMacroArgs> {
        match &self.kind {
            NodeKind::Macro { nodeargd, .. }
            | NodeKind::Environment { nodeargd, .. }
            | NodeKind::Specials { nodeargd, .. } => nodeargd.as_ref(),
            _ => None,
        }
    }

    /// `nodeoptarg` of a macro node (`pylatexenc 1.x` API): the optional argument, if the macro
    /// has one first.
    pub fn nodeoptarg(&self) -> Option<&LatexNode> {
        match &self.kind {
            NodeKind::Macro { nodeargd: Some(a), bare_expression: false, .. } => a.legacy_nodeoptarg_nodeargs().0,
            _ => None,
        }
    }

    /// `nodeargs` of a macro node (`pylatexenc 1.x` API): its arguments other than the optional
    /// one first. `None` for a macro read as the argument of another macro (where the attribute
    /// is `None` in Python).
    pub fn nodeargs(&self) -> Option<&[Option<LatexNode>]> {
        match &self.kind {
            NodeKind::Macro { bare_expression: true, .. } => None,
            NodeKind::Macro { nodeargd: Some(a), .. } => Some(a.legacy_nodeoptarg_nodeargs().1),
            NodeKind::Macro { nodeargd: None, .. } => Some(&[]),
            _ => None,
        }
    }

    /// `optargs` of an environment node: `[nodeoptarg]` (`pylatexenc 1.x` API).
    pub fn optargs(&self) -> Option<Vec<Option<&LatexNode>>> {
        match &self.kind {
            NodeKind::Environment { nodeargd, .. } => {
                Some(vec![nodeargd.as_ref().and_then(|a| a.legacy_nodeoptarg_nodeargs().0)])
            }
            _ => None,
        }
    }

    /// `args` of an environment node (`pylatexenc 1.x` API).
    pub fn args(&self) -> Option<&[Option<LatexNode>]> {
        match &self.kind {
            NodeKind::Environment { nodeargd, .. } => {
                Some(nodeargd.as_ref().map(|a| a.legacy_nodeoptarg_nodeargs().1).unwrap_or(&[]))
            }
            _ => None,
        }
    }

    pub fn is_chars(&self) -> bool {
        matches!(self.kind, NodeKind::Chars { .. })
    }
    pub fn is_group(&self) -> bool {
        matches!(self.kind, NodeKind::Group { .. })
    }
    pub fn is_comment(&self) -> bool {
        matches!(self.kind, NodeKind::Comment { .. })
    }
    pub fn is_macro(&self) -> bool {
        matches!(self.kind, NodeKind::Macro { .. })
    }
    pub fn is_environment(&self) -> bool {
        matches!(self.kind, NodeKind::Environment { .. })
    }
    pub fn is_specials(&self) -> bool {
        matches!(self.kind, NodeKind::Specials { .. })
    }
    pub fn is_math(&self) -> bool {
        matches!(self.kind, NodeKind::Math { .. })
    }
}
