use super::*;
use resonance_content::{arte, battle_rules as rules, battle_ui};

impl Publisher<'_> {
    pub fn battle_rules(&mut self, data: &rules::Captured) -> Result<Vec<String>> {
        self.battle_rules_under(data, "battle")
    }

    pub(crate) fn battle_rules_under(
        &mut self,
        data: &rules::Captured,
        root: &str,
    ) -> Result<Vec<String>> {
        library::validate_path(root)?;
        data.formations.validate()?;
        data.party.validate()?;
        data.defense.validate()?;
        data.unison.validate()?;
        data.techniques.validate()?;
        let normal = format!("{root}/party/normal-attacks.json");
        let martial = format!("{root}/party/techniques.json");
        let techniques = if root == "battle" {
            arte::PATH.into()
        } else {
            format!("{root}/techniques.json")
        };
        let documents = [
            (
                format!("{root}/formations.json"),
                serde_json::to_value(rules::Formations::from(data.formations.clone()))?,
            ),
            (
                normal.clone(),
                serde_json::to_value(&data.party.normal_attacks)?,
            ),
            (
                martial.clone(),
                serde_json::to_value(&data.party.martial_attacks)?,
            ),
            (
                format!("{root}/party/rules.json"),
                serde_json::to_value(rules::Party::from_table(
                    data.party.clone(),
                    normal,
                    martial,
                ))?,
            ),
            (
                format!("{root}/defense-system.json"),
                serde_json::to_value(rules::DefenseSystem::from(data.defense.clone()))?,
            ),
            (
                format!("{root}/unison.json"),
                serde_json::to_value(rules::Unison::from(data.unison.clone()))?,
            ),
            (
                format!("{root}/projectiles.json"),
                serde_json::to_value(rules::Projectiles::from(data.projectiles.clone()))?,
            ),
            (
                format!("{root}/recoil.json"),
                serde_json::to_value(rules::Recoil::from(data.recoil.clone()))?,
            ),
            (techniques, serde_json::to_value(&data.techniques)?),
        ];
        let mut paths = Vec::new();
        for (path, document) in documents {
            self.json(&path, &document)?;
            paths.push(path);
        }
        Ok(paths)
    }

    pub fn battle_ui(&mut self, art: battle_ui::Art) -> Result<Vec<String>> {
        self.battle_ui_under(art, "ui/battle", "fonts/battle")
    }

    pub(crate) fn battle_ui_under(
        &mut self,
        mut art: battle_ui::Art,
        root: &str,
        font: &str,
    ) -> Result<Vec<String>> {
        library::validate_path(root)?;
        library::validate_path(font)?;
        art.validate()?;
        art.font.texture = self.image(&art.font.texture, &format!("{font}/atlas"))?;
        let font = format!("{font}/font.json");
        self.json(&font, &art.font)?;
        let characters = [
            "lloyd", "colette", "genis", "raine", "sheena", "zelos", "presea", "regal", "kratos",
        ];
        for (portrait, name) in art.portraits.iter_mut().zip(characters) {
            if let Some(image) = portrait {
                image.path = self.image(&image.path, &format!("{root}/portraits/{name}"))?;
            }
        }
        let mut image =
            |image: &mut resonance_content::font::UiTexture, name: &str| -> Result<()> {
                image.path = self.image(&image.path, &format!("{root}/{name}"))?;
                Ok(())
            };
        image(&mut art.commands.texture, "commands/atlas")?;
        for (image_, name) in art
            .targets
            .iter_mut()
            .zip(["frame", "player-1", "player-2", "player-3", "player-4"])
        {
            image(image_, &format!("target/{name}"))?;
        }
        image(&mut art.stun, "stun")?;
        image(&mut art.notices.frame, "notices/frame")?;
        image(&mut art.notices.fill, "notices/fill")?;
        for (index, icon) in art.notices.icons.iter_mut().enumerate() {
            image(icon, &format!("notices/icon-{index}"))?;
        }
        for (face, name) in art.notices.faces.iter_mut().zip(characters) {
            image(face, &format!("notices/{name}"))?;
        }
        for (icon, name) in art.stat_icons.iter_mut().zip([
            "attack",
            "defense",
            "up",
            "magic-attack",
            "physical-barrier",
            "magical-barrier",
            "accuracy-curse",
            "reduced-item-effect",
            "holy-song",
            "down",
        ]) {
            image(icon, &format!("status/{name}"))?;
        }
        for (icon, name) in art.unison_buttons.iter_mut().zip(["a-b", "x-y"]) {
            image(icon, &format!("unison/{name}"))?;
        }
        // Scan icons retain their native atlas order, independently of the
        // contact-element ordering used by other battle tables.
        for (index, icon) in art.feedback.scan_icons.iter_mut().enumerate() {
            image(icon, &format!("scan/icon-{index:02}"))?;
        }
        image(&mut art.feedback.unison_frame, "unison/frame")?;
        for (icon, name) in art
            .feedback
            .escape
            .iter_mut()
            .zip(["frame", "needle", "fill"])
        {
            image(icon, &format!("escape/{name}"))?;
        }
        art.validate()?;
        let path = format!("{root}/battle.json");
        self.json(&path, &art.with_font(font.clone()))?;
        Ok(vec![path, font])
    }
}
