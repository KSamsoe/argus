//! String tables through the protocol (§12 "Dialogue and localization",
//! phase 3 item 4): `strings.list` reads them (a language's strings beside
//! the base's, or only those it lacks, a translator's to-do list), and
//! `edit.text` changes one in a transaction like any other edit, so the
//! change feed says who changed which words and undo takes them back.

use super::*;

impl Daemon {
    pub(super) fn strings_list(&self, params: &Json) -> Result<Json> {
        let ses = self.require_session()?;
        let ui = ses.ui.borrow();
        let st = &ui.strings;
        let lang = json::opt_str(params, "lang")?
            .unwrap_or(st.base())
            .to_string();
        let prefix = json::opt_str(params, "prefix")?.unwrap_or("");
        let missing = json::bool_param(params, "missing", false)?;
        let empty = std::collections::BTreeMap::new();
        let base = st.table(st.base()).unwrap_or(&empty);
        let mine = st.table(&lang);
        let mut keys: Vec<&String> = base.keys().collect();
        if let Some(m) = mine {
            keys.extend(m.keys().filter(|k| !base.contains_key(*k)));
        }
        keys.sort();
        keys.dedup();
        let rows: Vec<Json> = keys
            .into_iter()
            .filter(|k| k.starts_with(prefix))
            .map(|k| {
                let text = mine.and_then(|m| m.get(k));
                (k, base.get(k), text)
            })
            .filter(|(_, _, text)| !missing || text.is_none())
            .map(|(k, b, text)| json!([k, b, text]))
            .collect();
        let languages: Vec<Json> = st
            .languages()
            .iter()
            .map(|l| {
                let n = st.table(l).map_or(0, |t| t.len());
                json!({
                    "code": l,
                    "name": st.table(l).and_then(|t| t.get("language.name")),
                    "strings": n,
                    "missing": st.missing(l).len(),
                })
            })
            .collect();
        let (limit, offset) = query::page(params)?;
        let mut out = query::paged(rows, limit, offset);
        out["lang"] = json!(lang);
        out["base"] = json!(st.base());
        out["columns"] = json!(["key", st.base(), lang]);
        out["languages"] = json!(languages);
        if !st.problems().is_empty() {
            out["problems"] = json!(st.problems());
        }
        Ok(out)
    }

    pub(super) fn edit_text(&mut self, params: &Json) -> Result<Json> {
        let tx = json::u64_param(params, "tx", 0)? as u32;
        let lang = json::str_param(params, "lang")?.to_string();
        let key = json::str_param(params, "key")?.to_string();
        let text = match params.get("text") {
            None | Some(Json::Null) => None,
            Some(Json::String(s)) => Some(s.clone()),
            Some(_) => return Err(bad_params("text is a string, or null to remove the key")),
        };
        if lang.is_empty() || lang.contains(['/', '\\', '.']) {
            return Err(bad_params(format!("{lang:?} is not a language's code")));
        }
        let t = self.edits.tx_mut(tx)?;
        t.ops.push(Op::Text { lang, key, text });
        Ok(json!({ "queued": t.ops.len() }))
    }

    /// Write one string; returns the inverse (the text it had, or none).
    pub(super) fn apply_text(
        &mut self,
        op: &Op,
        touched: &mut BTreeSet<String>,
        files: &mut BTreeSet<String>,
    ) -> Result<Option<Op>> {
        let Op::Text { lang, key, text } = op else {
            return Ok(None);
        };
        let root = self.require_session()?.root.clone();
        let old = argus_ui::strings::set_in_file(&root, lang, key, text.as_deref())?;
        self.reload_strings();
        files.insert(format!("strings/{lang}.toml"));
        touched.insert(format!("strings/{lang}.toml#{key}"));
        Ok(Some(Op::Text {
            lang: lang.clone(),
            key: key.clone(),
            text: old,
        }))
    }

    /// Read the string tables again (after an edit, a reload).
    pub(super) fn reload_strings(&mut self) {
        let Some(ses) = self.session.as_ref() else {
            return;
        };
        let base = ProjectConfig::load(&ses.root)
            .map(|c| c.strings_base)
            .unwrap_or_else(|_| ses.ui.borrow().strings.base().to_string());
        let layers = argus_ecs::Layers::with_mods(&ses.root, &ses.mods.ids());
        ses.ui.borrow_mut().strings = argus_ui::Strings::load_layers(&layers, &base);
    }

    /// Read the dialogue graph again from the project's tables.
    pub(super) fn reload_dialogue(&mut self) {
        if let Some(ses) = self.session.as_ref() {
            *ses.dialogue.borrow_mut() = argus_dialogue::Graph::from_defs(&ses.project.defs);
        }
    }

    /// `script.errors` rows for dialogue rows and string tables that do not
    /// read.
    pub(super) fn text_errors(&self) -> Vec<Json> {
        let Some(ses) = self.session.as_ref() else {
            return Vec::new();
        };
        let split = |at: &str| match at.rsplit_once(':') {
            Some((f, l)) if l.parse::<u32>().is_ok() => (f.to_string(), l.parse::<u32>().ok()),
            _ => (at.to_string(), None),
        };
        let mut rows = Vec::new();
        for (at, msg) in &ses.dialogue.borrow().problems {
            let (file, line) = split(at);
            rows.push(json!({ "file": file, "line": line, "message": msg, "phase": "dialogue", "ref": at }));
        }
        for p in ses.ui.borrow().strings.problems() {
            let file = p.split(':').next().unwrap_or("strings").to_string();
            rows.push(json!({ "file": file, "line": null, "message": p, "phase": "strings", "ref": file }));
        }
        rows
    }
}
