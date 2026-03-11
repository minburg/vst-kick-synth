use nih_plug_vizia::vizia::prelude::*;

pub trait ModifiersExt {
    fn command(&self) -> bool;
    fn shift(&self) -> bool;
    fn alt(&self) -> bool;
}

impl ModifiersExt for Modifiers {
    fn command(&self) -> bool {
        self.contains(Modifiers::CTRL) || self.contains(Modifiers::LOGO)
    }

    fn shift(&self) -> bool {
        self.contains(Modifiers::SHIFT)
    }

    fn alt(&self) -> bool {
        self.contains(Modifiers::ALT)
    }
}

pub fn remap_current_entity_y_coordinate(cx: &mut EventContext, y: f32) -> f32 {
    let bounds = cx.bounds();
    let t = (y - bounds.y) / bounds.h;
    // Standard fader: 0.0 at bottom (t=1.0), 1.0 at top (t=0.0)
    (1.0 - t).clamp(0.0, 1.0)
}

pub fn remap_current_entity_y_t(cx: &mut EventContext, t: f32) -> f32 {
    let bounds = cx.bounds();
    // Inverse: y = bounds.y + (1.0 - value) * bounds.h
    bounds.y + (1.0 - t) * bounds.h
}
