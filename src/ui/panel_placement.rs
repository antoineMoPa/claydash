use super::*;
use egui_frames::{FrameId, LayoutNode, SplitDirection};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct PanelPlacements {
    closed: HashMap<EditorPane, PanelPlacement>,
}

struct PanelPlacement {
    frame: FrameId,
    following_tabs: Vec<PaneId>,
    neighbor: Option<(FrameId, DropSide)>,
}

impl PanelPlacements {
    pub(super) fn close(&mut self, layout: &mut Layout<EditorPane>, id: PaneId) {
        let Some(kind) = layout.pane(id).copied() else {
            return;
        };
        let Some(frame) = layout.frame_of(id) else {
            return;
        };
        let tabs = layout.frame(frame).expect("pane frame").panes();
        let index = tabs.iter().position(|tab| *tab == id).expect("pane tab");
        self.closed.insert(
            kind,
            PanelPlacement {
                frame,
                following_tabs: tabs[index + 1..].to_vec(),
                neighbor: adjacent_frame(layout.root(), frame),
            },
        );
        layout.close_pane(id);
    }

    pub(super) fn open(&mut self, layout: &mut Layout<EditorPane>, kind: EditorPane) {
        if let Some((id, _)) = layout.find_pane(|pane| *pane == kind) {
            layout.focus_pane(id);
            return;
        }
        if let Some(saved) = self.closed.remove(&kind) {
            if let Some(frame) = layout.frame(saved.frame) {
                let before = saved
                    .following_tabs
                    .iter()
                    .find(|id| frame.panes().contains(id))
                    .copied();
                layout.add_pane(saved.frame, kind, before);
                return;
            }
            if let Some((neighbor, side)) = saved.neighbor {
                if layout.frame(neighbor).is_some() {
                    let id = layout.add_pane_beside(neighbor, side, kind);
                    let restored = layout.frame_of(id).expect("restored panel frame");
                    // Other tabs closed from the same frame should rejoin it too.
                    for placement in self.closed.values_mut() {
                        if placement.frame == saved.frame {
                            placement.frame = restored;
                        }
                        if let Some((anchor, _)) = placement.neighbor.as_mut() {
                            if *anchor == saved.frame {
                                *anchor = restored;
                            }
                        }
                    }
                    return;
                }
            }
        }
        kind.open(layout);
    }
}

fn adjacent_frame(node: &LayoutNode, target: FrameId) -> Option<(FrameId, DropSide)> {
    let LayoutNode::Split {
        direction,
        children,
        ..
    } = node
    else {
        return None;
    };
    for (index, child) in children.iter().enumerate() {
        if let LayoutNode::Frame { frame } = child {
            if *frame == target {
                let (neighbor, before) = if index + 1 < children.len() {
                    (&children[index + 1], true)
                } else if index > 0 {
                    (&children[index - 1], false)
                } else {
                    return None;
                };
                let side = match (direction, before) {
                    (SplitDirection::Row, true) => DropSide::Left,
                    (SplitDirection::Row, false) => DropSide::Right,
                    (SplitDirection::Column, true) => DropSide::Top,
                    (SplitDirection::Column, false) => DropSide::Bottom,
                };
                let frames = neighbor.frames();
                return frames.first().copied().map(|frame| (frame, side));
            }
        }
        if let Some(placement) = adjacent_frame(child, target) {
            return Some(placement);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopened_tab_returns_to_its_moved_frame_and_tab_order() {
        let mut layout = Layout::with_pane(EditorPane::Viewport);
        let scene = layout.add_pane_against_edge(DropSide::Left, 0.3, EditorPane::Scene);
        let frame = layout.frame_of(scene).unwrap();
        let materials = layout.add_pane(frame, EditorPane::Materials, None);
        let world = layout.add_pane(frame, EditorPane::World, None);
        let mut placements = PanelPlacements::default();
        placements.close(&mut layout, materials);
        placements.open(&mut layout, EditorPane::Materials);
        let (reopened, _) = layout
            .find_pane(|pane| *pane == EditorPane::Materials)
            .unwrap();
        assert_eq!(
            layout.frame(frame).unwrap().panes(),
            &[scene, reopened, world]
        );
        assert_eq!(layout.active_pane().unwrap().0, reopened);
    }

    #[test]
    fn removed_frame_is_recreated_and_other_closed_tabs_rejoin_it() {
        let mut layout = Layout::with_pane(EditorPane::Viewport);
        let object = layout.add_pane_against_edge(DropSide::Top, 0.3, EditorPane::Object);
        let frame = layout.frame_of(object).unwrap();
        let world = layout.add_pane(frame, EditorPane::World, None);
        let mut placements = PanelPlacements::default();
        placements.close(&mut layout, world);
        placements.close(&mut layout, object);
        assert!(layout.frame(frame).is_none());
        placements.open(&mut layout, EditorPane::Object);
        placements.open(&mut layout, EditorPane::World);
        let (object, _) = layout
            .find_pane(|pane| *pane == EditorPane::Object)
            .unwrap();
        let (world, _) = layout.find_pane(|pane| *pane == EditorPane::World).unwrap();
        assert_eq!(layout.frame_of(object), layout.frame_of(world));
        let LayoutNode::Split {
            direction,
            children,
            ..
        } = layout.root()
        else {
            panic!("restored split")
        };
        assert_eq!(*direction, SplitDirection::Column);
        assert_eq!(children[0].frames(), vec![layout.frame_of(object).unwrap()]);
    }

    #[test]
    fn unavailable_previous_frame_falls_back_to_default_inspector_group() {
        let mut layout = Layout::with_pane(EditorPane::Viewport);
        let scene = layout.add_pane_against_edge(DropSide::Left, 0.3, EditorPane::Scene);
        let materials =
            layout.add_pane(layout.frame_of(scene).unwrap(), EditorPane::Materials, None);
        let mut placements = PanelPlacements::default();
        placements.close(&mut layout, materials);
        layout.close_pane(scene);
        EditorPane::Object.open(&mut layout);
        // Remove the saved neighboring frame as well, so no old anchor survives.
        let (viewport, _) = layout
            .find_pane(|pane| *pane == EditorPane::Viewport)
            .unwrap();
        layout.close_pane(viewport);
        placements.open(&mut layout, EditorPane::Materials);
        let (object, _) = layout
            .find_pane(|pane| *pane == EditorPane::Object)
            .unwrap();
        let (materials, _) = layout
            .find_pane(|pane| *pane == EditorPane::Materials)
            .unwrap();
        assert_eq!(layout.frame_of(materials), layout.frame_of(object));
    }
}
