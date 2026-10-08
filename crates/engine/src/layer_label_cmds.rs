//! Layer colour labels: Photoshop's Layers panel menu › No Color / Red / Orange / Yellow / Green /
//! Blue / Violet / Gray. The label is document data ([`photocraft_doc::Layer::label`], the PSD
//! `lclr` block, saved in `.pcraft` too); setting it is an undo step but changes no pixels.

use photocraft_doc::{LabelColor, LayerId};
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

pub const ID: &str = "layer.setColorLabel";

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: ID.into(), msg: msg.into() }
}

fn has_layer(s: &Session) -> std::result::Result<(), String> {
    let d = s.active().ok_or("no document open")?;
    d.active_layer.filter(|id| d.doc.layer(*id).is_some()).map(|_| ()).ok_or_else(|| "no active layer".into())
}

/// `{"layer":id?,"color":name}`: label one layer, or (no layer) every selected layer, as
/// Photoshop labels the whole selection from the right-click menu.
fn set_color_label(s: &mut Session, p: &Value) -> Result<Value> {
    let name = p.get("color").and_then(Value::as_str).ok_or_else(|| bad("missing `color` (none, red, orange, yellow, green, blue, violet or gray)"))?;
    let color = LabelColor::from_name(name).ok_or_else(|| bad(format!("unknown colour `{name}`")))?;
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let ids: Vec<LayerId> = match p.get("layer") {
        None | Some(Value::Null) => {
            let sel = st.selected_layers();
            if sel.is_empty() { st.active_layer.into_iter().collect() } else { sel }
        }
        Some(v) => vec![LayerId(v.as_u64().ok_or_else(|| bad("`layer` must be a layer id"))?)],
    };
    if ids.is_empty() {
        return Err(bad("no layer given and no active layer"));
    }
    for id in &ids {
        st.doc.layer(*id).ok_or(EngineError::NoLayer(*id))?;
    }
    if ids.iter().all(|id| st.doc.layer(*id).is_some_and(|l| l.label == color)) {
        return Ok(json!({"color": color.name(), "layers": 0}));
    }
    s.edit("Layer Properties", |doc, _| {
        for id in &ids {
            doc.layer_mut(*id).ok_or(EngineError::NoLayer(*id))?.label = color;
        }
        Ok(())
    })?;
    if let Some(st) = s.active_mut() {
        st.last_damage = Some(photocraft_geom::Rect::EMPTY);
    }
    Ok(json!({"color": color.name(), "layers": ids.len()}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: ID,
        label: "Layer Color",
        menu: &[],
        shortcut: None,
        params: r##"{"layer":id?,"color":"none|red|orange|yellow|green|blue|violet|gray"} (no layer: every selected layer)"##,
        enabled: has_layer,
        run: set_color_label,
        journal: true,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(s: &Session, id: u64) -> LabelColor {
        s.active().unwrap().doc.layer(LayerId(id)).unwrap().label
    }

    fn new_layer(s: &mut Session, name: &str) -> u64 {
        s.execute("layer.new.layer", json!({"name": name})).unwrap()["layer"].as_u64().unwrap()
    }

    #[test]
    fn sets_one_layer_with_an_undo_step_and_reports_it() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 32, "height": 32})).unwrap();
        let a = new_layer(&mut s, "A");
        let b = new_layer(&mut s, "B");
        let steps = s.active().unwrap().history.entries().len();
        assert_eq!(s.execute(ID, json!({"layer": a, "color": "red"})).unwrap()["layers"], 1);
        assert_eq!(label(&s, a), LabelColor::Red);
        assert_eq!(label(&s, b), LabelColor::None);
        assert_eq!(s.active().unwrap().history.entries().len(), steps + 1);
        assert_eq!(s.active().unwrap().last_damage, Some(photocraft_geom::Rect::EMPTY), "no pixels change");
        let inspect = s.execute("document.inspect", json!({})).unwrap();
        assert_eq!(inspect["layers"][1]["color"], "red");
        assert_eq!(inspect["layers"][0]["color"], "none");
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(label(&s, a), LabelColor::None);
    }

    #[test]
    fn without_a_layer_labels_every_selected_layer() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 32, "height": 32})).unwrap();
        let a = new_layer(&mut s, "A");
        let b = new_layer(&mut s, "B");
        let c = new_layer(&mut s, "C");
        s.execute("layer.select", json!({"layer": a})).unwrap();
        s.execute("layer.select", json!({"layer": b, "mode": "toggle"})).unwrap();
        assert_eq!(s.execute(ID, json!({"color": "Violet"})).unwrap()["layers"], 2);
        assert_eq!((label(&s, a), label(&s, b), label(&s, c)), (LabelColor::Violet, LabelColor::Violet, LabelColor::None));
        // Same colour again: nothing to do, no history step.
        let steps = s.active().unwrap().history.entries().len();
        assert_eq!(s.execute(ID, json!({"color": "violet"})).unwrap()["layers"], 0);
        assert_eq!(s.active().unwrap().history.entries().len(), steps);
        s.execute(ID, json!({"color": "none"})).unwrap();
        assert_eq!((label(&s, a), label(&s, b)), (LabelColor::None, LabelColor::None));
    }

    #[test]
    fn duplicates_keep_the_label() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 32, "height": 32})).unwrap();
        let a = new_layer(&mut s, "A");
        s.execute(ID, json!({"layer": a, "color": "green"})).unwrap();
        let d = s.execute("layer.duplicate", json!({"layer": a})).unwrap()["layer"].as_u64().unwrap();
        assert_eq!(label(&s, d), LabelColor::Green);
    }

    #[test]
    fn bad_params_fail_gracefully() {
        let mut s = Session::new();
        assert!(s.execute(ID, json!({"color": "red"})).is_err(), "no document");
        s.execute("file.new", json!({"width": 32, "height": 32})).unwrap();
        let a = new_layer(&mut s, "A");
        for p in [
            json!({}),
            json!({"color": 3}),
            json!({"color": "magenta"}),
            json!({"color": ""}),
            json!({"layer": "x", "color": "red"}),
            json!({"layer": -1, "color": "red"}),
            json!({"layer": u64::MAX, "color": "red"}),
            json!(null),
            json!([1, 2]),
        ] {
            assert!(s.execute(ID, p.clone()).is_err(), "{p} should fail");
        }
        assert_eq!(label(&s, a), LabelColor::None);
    }
}
