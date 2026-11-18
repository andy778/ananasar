//! View > SunSpec Device…, an Ananasar addition: finds a SunSpec map in the
//! register file and decodes every point by name, using SunSpec's JSON model
//! files from a folder the user chooses. The decoding rules are in
//! `ananasar_core::sunspec`; this module reads the model files and draws.

use crate::embed::SharedRegs;
use ananasar_core::registers::{Bank, RegisterFile};
use ananasar_core::sunspec::{
    self, Group, GroupCount, Model, ModelHeader, Placed, Point, PointType, ResolvedSf, ScaleFactor, Symbol, Value,
};
use ananasar_core::sync::lock;
use eframe::egui::{self, RichText};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::path::Path;

const SIZE: [f32; 2] = [760.0, 600.0];

#[derive(Default)]
pub struct SunSpecView {
    folder: String,
    bank: BankChoice,
    /// The loaded model definitions by model ID.
    pub(crate) models: BTreeMap<u16, Model>,
    /// What the last load did, for the status line.
    status: String,
}

/// The bank to search; holding registers unless the user picks input.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum BankChoice {
    #[default]
    Holding,
    Input,
}

impl BankChoice {
    fn bank(self) -> Bank {
        match self {
            BankChoice::Holding => Bank::Holding,
            BankChoice::Input => Bank::Input,
        }
    }
}

fn str_of<'a>(j: &'a Json, key: &str) -> Option<&'a str> {
    j.get(key).and_then(Json::as_str)
}

fn parse_point(j: &Json) -> Result<Point, String> {
    let name = str_of(j, "name").ok_or("a point has no name")?.to_owned();
    let type_name = str_of(j, "type").ok_or_else(|| format!("point {name} has no type"))?;
    let ty = PointType::from_name(type_name).ok_or_else(|| format!("point {name} has unknown type {type_name}"))?;
    let size = match j.get("size").and_then(Json::as_u64) {
        Some(s) => u16::try_from(s).map_err(|_| format!("point {name} is too large"))?,
        None => ty.fixed_size().ok_or_else(|| format!("point {name} has no size"))?,
    };
    if size == 0 {
        return Err(format!("point {name} has size 0"));
    }
    let sf = match j.get("sf") {
        Some(Json::String(s)) => Some(ScaleFactor::Point(s.clone())),
        Some(Json::Number(n)) => {
            let n = n.as_i64().and_then(|n| i8::try_from(n).ok()).filter(|n| (-10..=10).contains(n));
            Some(ScaleFactor::Fixed(n.ok_or_else(|| format!("point {name} has a scale factor outside -10..10"))?))
        }
        _ => None,
    };
    let symbols = j
        .get("symbols")
        .and_then(Json::as_array)
        .map(|syms| {
            syms.iter()
                .filter_map(|s| Some(Symbol { name: str_of(s, "name")?.to_owned(), value: s.get("value")?.as_u64()? }))
                .collect()
        })
        .unwrap_or_default();
    Ok(Point { name, ty, size, sf, units: str_of(j, "units").map(str::to_owned), symbols })
}

/// Model files nest groups only a few levels deep; the limit keeps a
/// hand-made file from recursing without end.
const MAX_DEPTH: u8 = 8;

fn parse_group(j: &Json, depth: u8) -> Result<Group, String> {
    if depth > MAX_DEPTH {
        return Err("groups are nested too deeply".into());
    }
    let name = str_of(j, "name").unwrap_or("group").to_owned();
    let count = match j.get("count") {
        None => GroupCount::Fixed(1),
        // A count of 0, or a point name, means "as many as the length allows".
        Some(Json::Number(n)) => match n.as_u64().and_then(|n| u16::try_from(n).ok()) {
            Some(0) => GroupCount::FromLength,
            Some(n) => GroupCount::Fixed(n),
            None => return Err(format!("group {name} has a bad count")),
        },
        Some(_) => GroupCount::FromLength,
    };
    let points = j
        .get("points")
        .and_then(Json::as_array)
        .map(|ps| ps.iter().map(parse_point).collect::<Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();
    let groups = j
        .get("groups")
        .and_then(Json::as_array)
        .map(|gs| gs.iter().map(|g| parse_group(g, depth.saturating_add(1))).collect::<Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();
    Ok(Group { name, count, points, groups })
}

/// A model from the text of a SunSpec JSON model file.
pub(crate) fn parse_model(text: &str) -> Result<Model, String> {
    let j: Json = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let id = j.get("id").and_then(Json::as_u64).and_then(|n| u16::try_from(n).ok()).ok_or("the model has no id")?;
    let g = j.get("group").ok_or("the model has no group")?;
    let mut group = parse_group(g, 0)?;
    group.count = GroupCount::Fixed(1);
    Ok(Model { id, label: str_of(g, "label").map(str::to_owned), group })
}

/// Every `*.json` model file in `dir`; returns the models and a status line.
fn load_dir(dir: &Path) -> (BTreeMap<u16, Model>, String) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => return (BTreeMap::new(), format!("Cannot read {}: {e}", dir.display())),
    };
    let mut models = BTreeMap::new();
    let mut failed = 0usize;
    for path in entries.filter_map(Result::ok).map(|e| e.path()) {
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| parse_model(&t)) {
            Ok(m) => {
                models.insert(m.id, m);
            }
            Err(_) => failed = failed.saturating_add(1),
        }
    }
    let status = if failed == 0 {
        format!("{} models loaded", models.len())
    } else {
        format!("{} models loaded, {failed} files could not be read as SunSpec models", models.len())
    };
    (models, status)
}

/// The decoded value of `p` from the register file.
fn value_of(regs: &RegisterFile, bank: Bank, p: &Placed<'_>) -> Option<Value> {
    let words: Vec<u16> = (0..p.point.size).map_while(|i| p.addr.checked_add(i)).map(|a| regs.get(bank, a)).collect();
    let sf = p.sf.map(|sf| match sf {
        ResolvedSf::Fixed(n) => Some(n),
        ResolvedSf::At(a) => sunspec::scale_factor(regs.get(bank, a)),
        ResolvedSf::Missing => None,
    });
    sunspec::decode(p.point, &words, sf)
}

/// One decoded row: point path, address, value and units.
pub(crate) type Row = (String, u16, String, String);

/// The decoded rows of the model at `at`, without its pad registers.
pub(crate) fn rows(regs: &RegisterFile, bank: Bank, model: &Model, at: ModelHeader) -> Vec<Row> {
    sunspec::layout(model, at)
        .iter()
        .filter(|p| p.point.ty != PointType::Pad)
        .map(|p| {
            let value = value_of(regs, bank, p);
            // Units mean nothing next to a value the device does not have.
            let units = match value {
                Some(Value::NotImplemented) | None => String::new(),
                Some(_) => p.point.units.clone().unwrap_or_default(),
            };
            (p.path.clone(), p.addr, value.map_or_else(|| "—".to_owned(), |v| v.to_string()), units)
        })
        .collect()
}

impl SunSpecView {
    /// Show the window; `open` is cleared when it is closed.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool, regs: &SharedRegs) {
        if !*open {
            return;
        }
        let closed = crate::dialog::show(ctx, "sunspec-device", "SunSpec Device", SIZE, 8, &mut |ui| self.ui(ui, regs));
        if closed {
            *open = false;
        }
    }

    fn load(&mut self) {
        let (models, status) = load_dir(Path::new(self.folder.trim()));
        self.models = models;
        self.status = status;
    }

    fn ui(&mut self, ui: &mut egui::Ui, regs: &SharedRegs) {
        ui.horizontal(|ui| {
            let label = ui.label("Models folder");
            ui.add(egui::TextEdit::singleline(&mut self.folder).desired_width(380.0)).labelled_by(label.id);
            if ui.button("Browse…").clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                self.folder = dir.display().to_string();
                self.load();
            }
            if ui.button("Load").clicked() {
                self.load();
            }
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.bank, BankChoice::Holding, "Holding");
            ui.radio_value(&mut self.bank, BankChoice::Input, "Input");
            ui.add_space(12.0);
            if self.status.is_empty() {
                ui.label("Choose a folder of SunSpec JSON model files (json/ in github.com/sunspec/models).");
            } else {
                ui.label(&self.status);
            }
        });
        ui.separator();

        let bank = self.bank.bank();
        let r = lock(regs);
        let Some(dev) = sunspec::discover(|a| r.get(bank, a)) else {
            ui.label("No SunSpec marker (SunS) at 40000, 0 or 50000 in this bank.");
            return;
        };
        let mut head = format!(
            "SunS at {} (register {}), {} models",
            dev.base,
            u32::from(dev.base).saturating_add(1),
            dev.models.len()
        );
        if !dev.complete {
            head.push_str(", no end model before the end of the bank");
        }
        ui.strong(head);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for at in &dev.models {
                let Some(model) = self.models.get(&at.id) else {
                    ui.label(format!("Model {} at {}, L={}: no model file loaded", at.id, at.addr, at.len));
                    continue;
                };
                let title = format!(
                    "Model {} · {} · at {}, L={}",
                    at.id,
                    model.label.as_deref().unwrap_or("(no label)"),
                    at.addr,
                    at.len
                );
                egui::CollapsingHeader::new(title).id_salt(at.addr).default_open(true).show(ui, |ui| {
                    egui::Grid::new(("sunspec", at.addr)).striped(true).num_columns(4).show(ui, |ui| {
                        for h in ["Point", "Address", "Value", "Units"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for (path, addr, value, units) in rows(&r, bank, model, *at) {
                            ui.label(path);
                            ui.label(RichText::new(addr.to_string()).monospace());
                            ui.label(RichText::new(value).monospace());
                            ui.label(units);
                            ui.end_row();
                        }
                    });
                });
            }
        });
        drop(r);
        // The registers can change under the window (the server keeps serving).
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small model in SunSpec's JSON layout, written for these tests: a
    /// scaled value, an enum, a bitfield, a string, a pad and a repeating group.
    pub(crate) const TEST_MODEL: &str = r#"{
        "id": 64900,
        "group": {
            "name": "test", "label": "Test Model", "type": "group",
            "points": [
                {"name": "ID", "type": "uint16", "size": 1, "value": 64900},
                {"name": "L", "type": "uint16", "size": 1},
                {"name": "W", "type": "int16", "size": 1, "sf": "W_SF", "units": "W"},
                {"name": "W_SF", "type": "sunssf", "size": 1},
                {"name": "Hz", "type": "uint16", "size": 1, "sf": -2, "units": "Hz"},
                {"name": "St", "type": "enum16", "size": 1,
                 "symbols": [{"name": "OFF", "value": 1}, {"name": "MPPT", "value": 4}]},
                {"name": "Evt", "type": "bitfield32", "size": 2,
                 "symbols": [{"name": "GROUND_FAULT", "value": 0}, {"name": "OVER_TEMP", "value": 3}]},
                {"name": "Opt", "type": "string", "size": 4},
                {"name": "Pad", "type": "pad", "size": 1}
            ],
            "groups": [{
                "name": "string", "type": "group", "count": 0,
                "points": [
                    {"name": "DCA", "type": "uint16", "size": 1, "sf": "W_SF", "units": "A"},
                    {"name": "Tmp", "type": "int16", "size": 1, "units": "C"}
                ]
            }]
        }
    }"#;

    #[test]
    fn a_model_file_parses() {
        let m = parse_model(TEST_MODEL).unwrap();
        assert_eq!((m.id, m.label.as_deref()), (64900, Some("Test Model")));
        assert_eq!(m.group.points.len(), 9);
        assert_eq!(m.group.points[2].sf, Some(ScaleFactor::Point("W_SF".into())));
        assert_eq!(m.group.points[4].sf, Some(ScaleFactor::Fixed(-2)));
        assert_eq!(m.group.points[6].size, 2);
        assert_eq!(m.group.points[5].symbols[1], Symbol { name: "MPPT".into(), value: 4 });
        assert_eq!(m.group.groups[0].count, GroupCount::FromLength);
    }

    #[test]
    fn bad_model_files_are_errors_not_panics() {
        for bad in [
            "",
            "[]",
            r#"{"id": 1}"#,
            r#"{"id": 70000, "group": {}}"#,
            r#"{"id": 1, "group": {"points": [{"name": "x", "type": "int128"}]}}"#,
            r#"{"id": 1, "group": {"points": [{"name": "x", "type": "string"}]}}"#,
            r#"{"id": 1, "group": {"points": [{"name": "x", "type": "int16", "sf": 99}]}}"#,
            r#"{"id": 1, "group": {"points": [{"name": "x", "type": "int16", "size": 0}]}}"#,
        ] {
            assert!(parse_model(bad).is_err(), "{bad}");
        }
        let mut group = "{}".to_owned();
        for _ in 0..20 {
            group = format!(r#"{{"groups":[{group}]}}"#);
        }
        assert_eq!(parse_model(&format!(r#"{{"id":1,"group":{group}}}"#)), Err("groups are nested too deeply".into()));
    }

    /// The register file of a small SunSpec device: the common model with a
    /// manufacturer name, then the test model with two repeating instances.
    pub(crate) fn device() -> RegisterFile {
        let mut r = RegisterFile::new();
        let mut put = |addr: u16, vals: &[u16]| {
            for (a, v) in (addr..).zip(vals) {
                r.set_holding(a, *v);
            }
        };
        put(40000, &[0x5375, 0x6E53]);
        // Common model 1, L = 66: Mn "ACME" at offset 2.
        put(40002, &[1, 66, 0x4143, 0x4D45]);
        // Test model at 40070: a fixed part of 11 registers, then two instances of 2.
        put(40070, &[64900, 15, 4520, 0xFFFF, 5003, 4, 0, 0b1001, 0x4142, 0x4300, 0, 0, 0x8000]);
        put(40083, &[123, 25, 0xFFFF, 0x8000]);
        put(40087, &[0xFFFF, 0]);
        r
    }

    #[test]
    fn the_test_device_decodes() {
        let m = parse_model(TEST_MODEL).unwrap();
        let r = device();
        let dev = sunspec::discover(|a| r.get(Bank::Holding, a)).unwrap();
        assert!(dev.complete);
        let at = dev.models[1];
        assert_eq!((at.id, at.addr, at.len), (64900, 40070, 15));
        let got: Vec<_> =
            rows(&r, Bank::Holding, &m, at).into_iter().map(|(p, _, v, u)| format!("{p}={v}{u}")).collect();
        assert_eq!(
            got,
            [
                "ID=64900",
                "L=15",
                "W=452.0W",
                "W_SF=-1",
                "Hz=50.03Hz",
                "St=MPPT (4)",
                "Evt=GROUND_FAULT, OVER_TEMP (0x9)",
                "Opt=ABC",
                "string[1].DCA=12.3A",
                "string[1].Tmp=25C",
                "string[2].DCA=n/a",
                "string[2].Tmp=n/a",
            ]
        );
    }
}
