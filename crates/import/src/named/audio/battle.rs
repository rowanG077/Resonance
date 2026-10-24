use super::*;
use resonance_content::battle_audio::document as battle;

impl Publisher<'_> {
    /// Publish the full physical catalogue, including songs and cues not selected
    /// by any field. Original native bank/archive identities determine ownership.
    pub fn battle_audio(&mut self) -> Result<()> {
        let source = self.audio_source()?;
        let music = source
            .music_ids()
            .into_iter()
            .map(u16::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let speech: Vec<_> = source
            .speech
            .iter()
            .filter(|(_, archive)| archive.source.is_some())
            .flat_map(|(&base, archive)| {
                archive
                    .members
                    .iter()
                    .enumerate()
                    .filter_map(move |(id, member)| (!member.empty).then_some(base | id as u32))
            })
            .collect();
        self.music_recordings(music, &|| false)?;
        self.sound_catalogue()?;
        self.speech_lines(speech)?;
        let source = self.audio_source()?;
        let mut archive_playback = BTreeMap::new();
        let mut voice_playback = BTreeMap::new();
        for (&base, archive) in &source.speech {
            if archive.source.is_none() {
                continue;
            }
            let rates: BTreeSet<_> = archive
                .members
                .iter()
                .filter(|member| !member.empty)
                .map(|member| member.playback.sample_rate)
                .collect();
            if rates.len() == 1 {
                if let Some(rate) = rates.first().copied().flatten() {
                    archive_playback.insert(
                        base,
                        Playback {
                            sample_rate: Some(rate),
                        },
                    );
                }
            } else {
                for (member, voice) in archive.members.iter().enumerate() {
                    if !voice.empty && voice.playback.sample_rate.is_some() {
                        voice_playback.insert(base | member as u32, voice.playback.clone());
                    }
                }
            }
        }
        let environments = std::iter::once(source)
            .chain(source.additional.as_deref())
            .map(|source| -> Result<_> {
                Ok((
                    source.disc,
                    source
                        .streams
                        .context("missing native battle stream owner")?,
                    source.original.music_reverbs()?,
                    source.original.voice_gains()?,
                    source.battle_reverbs,
                    source.original.battle_spatial()?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        for (disc, streams, music_reverbs, voice_gains, reverbs, [effect_spatial, voice_spatial]) in
            environments
        {
            let definition = battle::Definition {
                version: battle::VERSION,
                music: "audio/music/mappings.json".into(),
                effects: "audio/effects/mappings.json".into(),
                streams: Some(streams),
                archive_playback: archive_playback.clone(),
                voice_playback: voice_playback.clone(),
                music_reverbs,
                voice_gains,
                reverbs,
                effect_spatial,
                voice_spatial,
            };
            self.json(battle::PATH, &definition)
                .with_context(|| format!("publish shared battle audio from source disc {disc}"))?;
        }
        // Coverage reports describe native source availability, never runtime
        // dependencies or a second catalogue that authors must maintain.
        let mut archives = BTreeMap::new();
        for (&base, archive) in &self.audio_source()?.speech {
            let name = &archive.name;
            let empty: Vec<_> = archive
                .members
                .iter()
                .enumerate()
                .filter_map(|(id, member)| member.empty.then_some(id))
                .collect();
            let available = archive.source.is_some();
            let count = self
                .mappings
                .get(&format!("audio/speech/{name}/mappings.json"))
                .map_or(0, |m| m.len());
            ensure!(
                !available || count + empty.len() == archive.members.len(),
                "named speech coverage differs for {name}"
            );
            archives.insert(
                base,
                serde_json::json!({"name": name, "available":available,
                "declared_members":archive.members.len(), "playable_members":count, "empty_members":empty}),
            );
        }
        self.json("reports/audio-speech.json", &archives)
    }
}
