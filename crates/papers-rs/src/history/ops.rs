//! The file operations of a command (spec/history.md, section 4): what it
//! moved, copied, deleted. A state keeps them, so that undo can put the
//! files back.

use super::ojson::J;
use crate::config::paths::{abspath, basename, dirname, is_abs, join, normpath, relpath};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// A file or folder renamed or moved from `src` to `dst`.
    Moved,
    /// A file copied (or hard-linked) from `src` to `dst`.
    Copied,
    /// A file deleted by the command and kept in the store (by its checksum).
    Trashed,
    /// A file deleted and not kept (the system trash, or deleted); with a
    /// `dst`, an identical file stays there.
    Removed,
}

impl OpKind {
    pub fn name(self) -> &'static str {
        match self {
            OpKind::Moved => "moved",
            OpKind::Copied => "copied",
            OpKind::Trashed => "trashed",
            OpKind::Removed => "removed",
        }
    }

    pub fn from_name(name: &str) -> Option<OpKind> {
        match name {
            "moved" => Some(OpKind::Moved),
            "copied" => Some(OpKind::Copied),
            "trashed" => Some(OpKind::Trashed),
            "removed" => Some(OpKind::Removed),
            _ => None,
        }
    }
}

/// A file operation. The paths are absolute and normalized in the operations
/// of a command (the hook of the library gives them so); a state stores them
/// relative to the directory of the bibtex when they are inside it (see
/// `relative_ops`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOp {
    pub kind: OpKind,
    pub src: String,
    pub dst: Option<String>,
    /// The sha256 of the file (none for a folder).
    pub checksum: Option<String>,
}

impl FileOp {
    /// An operation of a command: the paths are made absolute (`abspath`).
    pub fn new(kind: OpKind, src: &str, dst: Option<&str>, checksum: Option<String>) -> FileOp {
        FileOp {
            kind,
            src: abspath(src),
            dst: dst.map(abspath),
            checksum,
        }
    }

    pub fn to_j(&self) -> J {
        J::Obj(vec![
            ("op".into(), J::str(self.kind.name())),
            ("src".into(), J::str(&self.src)),
            ("dst".into(), J::opt_str(&self.dst)),
            ("checksum".into(), J::opt_str(&self.checksum)),
        ])
    }

    pub fn from_j(j: &J) -> Option<FileOp> {
        let kind = OpKind::from_name(j.get("op")?.as_str()?)?;
        Some(FileOp {
            kind,
            src: j.get("src")?.as_str()?.to_string(),
            dst: j.get("dst").and_then(J::as_str).map(str::to_string),
            checksum: j
                .get("checksum")
                .and_then(J::as_str)
                .filter(|c| !c.is_empty())
                .map(str::to_string),
        })
    }

    fn map_paths(&self, f: &dyn Fn(&str) -> String) -> FileOp {
        FileOp {
            kind: self.kind,
            src: f(&self.src),
            dst: self.dst.as_deref().filter(|d| !d.is_empty()).map(f),
            checksum: self.checksum.clone(),
        }
    }
}

/// The paths of the operations as a library writes its links: relative to
/// the directory of the bibtex for a file inside it (relative paths), else
/// absolute. `libdir` is the directory of the bibtex.
pub fn relative_ops(ops: &[FileOp], libdir: &str, absolute_paths: bool) -> Vec<FileOp> {
    if absolute_paths {
        return ops.to_vec();
    }
    let rel = |path: &str| {
        if !path.is_empty() && is_abs(path) {
            let r = relpath(path, libdir);
            if r != ".." && !r.starts_with("../") {
                return r;
            }
        }
        path.to_string()
    };
    ops.iter().map(|op| op.map_paths(&rel)).collect()
}

/// The operations of a state with absolute normalized paths: a relative path
/// is resolved against the directory of the bibtex of the library.
pub fn absolute_ops(ops: &[FileOp], libdir: &str) -> Vec<FileOp> {
    let ab = |path: &str| {
        if !path.is_empty() && !is_abs(path) {
            normpath(&join(libdir, path))
        } else {
            path.to_string()
        }
    };
    ops.iter().map(|op| op.map_paths(&ab)).collect()
}

/// A file of the files deleted by the commands (`file-objects/ab/ab12….pdf`), in any store: the
/// name of the folder above the folder of the path is `file-objects`, and the file name starts
/// with the name of its folder.
pub fn is_file_object(path: &str) -> bool {
    let folder = dirname(path);
    basename(&dirname(&folder)) == "file-objects" && basename(path).starts_with(&basename(&folder))
}

/// The sorted checksums, without repeats, of the file objects that a state needs (`deleted`): those
/// of the `trashed` operations, and those of the `copied` operations whose `src` is a file object
/// (a file brought back from the history, spec 3.4).
pub fn deleted_refs(ops: &[FileOp]) -> Vec<String> {
    let mut found: Vec<String> = ops
        .iter()
        .filter(|op| {
            op.kind == OpKind::Trashed || op.kind == OpKind::Copied && is_file_object(&op.src)
        })
        .filter_map(|op| op.checksum.clone())
        .collect();
    found.sort();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_inside_the_library_are_relative() {
        let ops = vec![
            FileOp::new(
                OpKind::Moved,
                "/lib/dl/x.pdf",
                Some("/lib/files/a.pdf"),
                Some("c".into()),
            ),
            FileOp::new(
                OpKind::Copied,
                "/home/u/x.pdf",
                Some("/lib/../lib/files/b.pdf"),
                None,
            ),
            FileOp::new(OpKind::Trashed, "/lib/..x/y.pdf", None, Some("d".into())),
        ];
        let rel = relative_ops(&ops, "/lib", false);
        assert_eq!(rel[0].src, "dl/x.pdf");
        assert_eq!(rel[0].dst.as_deref(), Some("files/a.pdf"));
        assert_eq!(rel[1].src, "/home/u/x.pdf");
        assert_eq!(rel[1].dst.as_deref(), Some("files/b.pdf"));
        assert_eq!(rel[2].src, "..x/y.pdf");
        assert_eq!(relative_ops(&ops, "/lib", true), ops);
        assert_eq!(absolute_ops(&rel, "/lib")[0], ops[0]);
        assert_eq!(deleted_refs(&ops), vec!["d".to_string()]);
        // a file brought back from the history is kept by the step that copied it
        assert!(is_file_object("/h/file-objects/ab/ab12cd.pdf"));
        assert!(!is_file_object("/h/file-objects/ab/cd12ab.pdf"));
        assert!(!is_file_object("/h/objects/ab/ab12cd"));
        let back = FileOp::new(
            OpKind::Copied,
            "/h/file-objects/ab/ab12cd.pdf",
            Some("/lib/a.pdf"),
            Some("ab12cd".into()),
        );
        let plain = FileOp::new(
            OpKind::Copied,
            "/x/a.pdf",
            Some("/lib/b.pdf"),
            Some("ee".into()),
        );
        assert_eq!(deleted_refs(&[back, plain]), vec!["ab12cd".to_string()]);
    }
}
