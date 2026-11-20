//! Project-wide find and replace.
//!
//! A plain-text search is planned into a [`ReplacePlan`] (nothing on disk
//! changes), the dialog confirms it, and then every file is committed as one
//! guarded transaction — the same staging, recovery and history snapshots the
//! Note Composer uses. Operators, regex and multi-word searches have no single
//! literal text, so they cannot be replaced.

use super::*;
use crate::search_query;
use crate::search_service::ReplacePlan;

impl Workspace {
    /// Build a replacement plan for a plain-text query, or a localized error
    /// message for the dialog. Read-only: no file is written here.
    pub fn plan_replace_in_project(
        &self,
        query: &str,
        replacement: &str,
        match_case: bool,
        cx: &App,
    ) -> Result<ReplacePlan, String> {
        let Some((literal, case_override)) = search_query::parse_query(query)
            .ok()
            .as_ref()
            .and_then(search_query::literal_term)
            .filter(|(text, _)| !text.is_empty())
        else {
            return Err(self
                .tr(
                    "Replace needs a single plain-text search — one word or a quoted phrase.",
                    "Erstatt krever et enkelt tekstsøk — ett ord eller en sitert fras.",
                )
                .to_string());
        };
        let match_case = case_override.unwrap_or(match_case);
        let paths = self.vault.read(cx).explorer_path_snapshot();
        let plan = self
            .search_service
            .plan_replacements(&paths, &literal, replacement, match_case);
        if plan.is_empty() {
            return Err(self
                .tr("No occurrences to replace", "Ingen forekomster å erstatte")
                .to_string());
        }
        let changes = plan
            .files
            .iter()
            .map(|file| (file.path.clone(), file.before.clone(), file.after.clone()))
            .collect::<Vec<_>>();
        if self.composer_change_conflicts(&changes, cx) {
            return Err(self
                .tr(
                    "Save or close the open notes that changed before replacing.",
                    "Lagre eller lukk de åpne notatene som ble endret før du erstatter.",
                )
                .to_string());
        }
        Ok(plan)
    }

    /// Commit a plan: snapshot each file's history, write every change as one
    /// guarded transaction, and bring open buffers in this and peer windows up
    /// to date. Returns a localized success summary or an error message.
    pub fn apply_replace_in_project(
        &mut self,
        plan: &ReplacePlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return Err(self
                .tr(
                    "Open a folder to replace in project",
                    "Åpne en mappe for å erstatte i prosjektet",
                )
                .to_string());
        };
        let language = self.settings.language;
        let changes = plan
            .files
            .iter()
            .map(|file| (file.path.clone(), file.before.clone(), file.after.clone()))
            .collect::<Vec<_>>();
        for (path, before, after) in &changes {
            if crate::history::snapshot_before_write_checked(&root, path, before, after).is_err() {
                return Err(self
                    .tr(
                        "Could not create a history snapshot; the replace was cancelled.",
                        "Kunne ikke opprette et historikkøyeblikksbilde; erstatningen ble avbrutt.",
                    )
                    .to_string());
            }
        }
        let transaction: Vec<crate::note_composer::ComposerTransactionChange> = changes
            .iter()
            .map(|(path, before, after)| (path.clone(), Some(before.clone()), Some(after.clone())))
            .collect();
        if let Err(error) = crate::note_composer::merge_transaction_changes(&root, &transaction) {
            return Err(error.message(language == Language::Norwegian).to_string());
        }
        self.apply_composer_changes(&changes, window, cx);
        let peers = cx.global::<crate::windows::WindowManager>().records.clone();
        for peer in peers.iter().filter(|peer| peer.id != self.window_id) {
            let _ = peer.window.update(cx, |_, window, cx| {
                peer.workspace.update(cx, |workspace, cx| {
                    workspace.apply_composer_changes(&changes, window, cx);
                    workspace.vault.update(cx, |vault, cx| vault.refresh(cx));
                })
            });
        }
        self.search_service.invalidate_all();
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
        let files = changes.len();
        Ok(if language == Language::Norwegian {
            format!("Erstattet {} forekomster i {} notater", plan.total, files)
        } else {
            format!("Replaced {} occurrences across {} notes", plan.total, files)
        })
    }
}
