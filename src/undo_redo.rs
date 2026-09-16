use crate::model::DataTree;

pub const UNDO_SHORTCUT: &str = "Shift+Z";
pub const REDO_SHORTCUT: &str = "Shift+Y";

pub fn undo(tree: &mut DataTree) {
    tree.undo();
}

pub fn redo(tree: &mut DataTree) {
    tree.redo();
}
