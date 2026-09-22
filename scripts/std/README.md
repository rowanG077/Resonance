# SymphoniaScript standard library

These checked-in constants are the source of truth. The build embeds them; cooking
copies the files unchanged. Edit these sources, rebuild and recook. Cooked resources
are immutable; mod overlays are future work. Additional constants need no enum or
native registration: use ordinary `pub const Name: string = "...";` or `i32`.

## Catalogues

- [`std::story`](story.sym)
- [`std::characters`](characters.sym)
- [`std::items`](items.sym)
- [`std::monsters`](monsters.sym)
- [`std::figurines`](figurines.sym)
- [`std::locations`](locations.sym)

Names retain original IDs and model node selectors. Unknown meanings are explicitly
labelled. `std::characters::Lloyd` is canonical actor ID 1. `std::story` names
2,048 flag IDs; additional constants can use the full `u16` range.

## Model nodes

Monsters and figurines share the model and animation machinery. Their catalogue IDs
are distinct. These modules share node names for identical ordered skeleton names;
that does not imply identical meshes, materials or animations. Models without a
known catalogue name retain a `model_<hash>` module. The `Skeleton` comment links
a module to its ordered bone-name interface for cooker documentation, not runtime
resource validation. New models remain cookable without a standard-library entry.

Use `model::node(sword_dancer_191::LeftWing)` after importing
`std::nodes::sword_dancer_191`. Node constants are plain strings; custom constants
and literal strings work identically. Duplicate node labels use zero-based `#0`,
`#1` occurrence suffixes; a literal `#` in a label becomes `##`.

| Catalogue | ID | Name | Node constants |
| --- | --- | --- | --- |
| monsters | 0 | Torent | [torent_000](nodes/torent_000.sym) |
| monsters | 1 | Orcrot | [torent_000](nodes/torent_000.sym) |
| monsters | 2 | Marcroid | [marcroid_002](nodes/marcroid_002.sym) |
| monsters | 3 | Minicoid | [minicoid_003](nodes/minicoid_003.sym) |
| monsters | 4 | Tentacle Plant | [tentacle_plant_004](nodes/tentacle_plant_004.sym) |
| monsters | 5 | Mocking Plant | [tentacle_plant_004](nodes/tentacle_plant_004.sym) |
| monsters | 6 | Mandragora | [mandragora_006](nodes/mandragora_006.sym) |
| monsters | 7 | Alraune | [mandragora_006](nodes/mandragora_006.sym) |
| monsters | 8 | Insect Plant | [insect_plant_008](nodes/insect_plant_008.sym) |
| monsters | 9 | Carnivorous Plant | [insect_plant_008](nodes/insect_plant_008.sym) |
| monsters | 10 | Bomb Plant | [bomb_plant_010](nodes/bomb_plant_010.sym) |
| monsters | 11 | Bomb Seedling | [bomb_seedling_011](nodes/bomb_seedling_011.sym) |
| monsters | 12 | Pumpkin Tree | [pumpkin_tree_012](nodes/pumpkin_tree_012.sym) |
| monsters | 13 | Bellpepper Head | [pumpkin_tree_012](nodes/pumpkin_tree_012.sym) |
| monsters | 14 | Boxer Iris | [boxer_iris_014](nodes/boxer_iris_014.sym) |
| monsters | 15 | Evil Orchid | [boxer_iris_014](nodes/boxer_iris_014.sym) |
| monsters | 16 | Poison Lily | [poison_lily_016](nodes/poison_lily_016.sym) |
| monsters | 17 | Wolf | [wolf_017](nodes/wolf_017.sym) |
| monsters | 18 | Night Raid | [wolf_017](nodes/wolf_017.sym) |
| monsters | 19 | Bear | [bear_019](nodes/bear_019.sym) |
| monsters | 20 | Egg Bear | [bear_019](nodes/bear_019.sym) |
| monsters | 21 | Rabbit | [rabbit_021](nodes/rabbit_021.sym) |
| monsters | 22 | Hare | [rabbit_021](nodes/rabbit_021.sym) |
| monsters | 23 | Bigfoot | [bigfoot_023](nodes/bigfoot_023.sym) |
| monsters | 24 | Sidewinder | [sidewinder_024](nodes/sidewinder_024.sym) |
| monsters | 25 | Violent Viper | [sidewinder_024](nodes/sidewinder_024.sym) |
| monsters | 26 | Manticore | [manticore_026](nodes/manticore_026.sym) |
| monsters | 27 | Chimaera | [chimaera_027](nodes/chimaera_027.sym) |
| monsters | 28 | Lobo | [lobo_028](nodes/lobo_028.sym) |
| monsters | 29 | Sasquatch | [bigfoot_023](nodes/bigfoot_023.sym) |
| monsters | 30 | Boar | [boar_030](nodes/boar_030.sym) |
| monsters | 31 | Baby Boar | [baby_boar_031](nodes/baby_boar_031.sym) |
| monsters | 32 | Basilisk | [basilisk_032](nodes/basilisk_032.sym) |
| monsters | 33 | Sewer Rat | [sewer_rat_033](nodes/sewer_rat_033.sym) |
| monsters | 34 | Sewer Rat | [sewer_rat_034](nodes/sewer_rat_034.sym) |
| monsters | 35 | Armaboar | [boar_030](nodes/boar_030.sym) |
| monsters | 36 | Zombie | [zombie_036](nodes/zombie_036.sym) |
| monsters | 37 | Ghoul | [zombie_036](nodes/zombie_036.sym) |
| monsters | 38 | Demon | [demon_038](nodes/demon_038.sym) |
| monsters | 39 | Arch Demon | [demon_038](nodes/demon_038.sym) |
| monsters | 40 | Skeleton | [skeleton_040](nodes/skeleton_040.sym) |
| monsters | 41 | Gold Skeleton | [skeleton_040](nodes/skeleton_040.sym) |
| monsters | 42 | Undertaker | [undertaker_042](nodes/undertaker_042.sym) |
| monsters | 43 | Coffinmaster | [coffinmaster_043](nodes/coffinmaster_043.sym) |
| monsters | 44 | Living Armor | [living_armor_044](nodes/living_armor_044.sym) |
| monsters | 45 | Specter | [specter_045](nodes/specter_045.sym) |
| monsters | 46 | Phantasm | [phantasm_046](nodes/phantasm_046.sym) |
| monsters | 47 | Death | [death_047](nodes/death_047.sym) |
| monsters | 48 | Grim Reaper | [grim_reaper_048](nodes/grim_reaper_048.sym) |
| monsters | 49 | Ghost | [ghost_049](nodes/ghost_049.sym) |
| monsters | 50 | Phantom | [phantom_050](nodes/phantom_050.sym) |
| monsters | 51 | Lamia | [lamia_051](nodes/lamia_051.sym) |
| monsters | 52 | Medusa | [medusa_052](nodes/medusa_052.sym) |
| monsters | 53 | Doom Guard | [doom_guard_053](nodes/doom_guard_053.sym) |
| monsters | 54 | Phantom Knight | [phantom_knight_054](nodes/phantom_knight_054.sym) |
| monsters | 55 | Hell Knight | [hell_knight_055](nodes/hell_knight_055.sym) |
| monsters | 56 | Samael | [samael_056](nodes/samael_056.sym) |
| monsters | 57 | Pharaoh Knight | [skeleton_040](nodes/skeleton_040.sym) |
| monsters | 58 | Golem | [golem_058](nodes/golem_058.sym) |
| monsters | 59 | Rock Golem | [rock_golem_059](nodes/rock_golem_059.sym) |
| monsters | 60 | Clay Golem | [clay_golem_060](nodes/clay_golem_060.sym) |
| monsters | 61 | Gentleman | [gentleman_061](nodes/gentleman_061.sym) |
| monsters | 62 | Living Doll | [living_doll_062](nodes/living_doll_062.sym) |
| monsters | 63 | Evil Teddy | [evil_teddy_063](nodes/evil_teddy_063.sym) |
| monsters | 64 | Living Sword | [living_sword_064](nodes/living_sword_064.sym) |
| monsters | 65 | Melting Pot | [melting_pot_065](nodes/melting_pot_065.sym) |
| monsters | 66 | Brown Pot | [brown_pot_066](nodes/brown_pot_066.sym) |
| monsters | 67 | Fire Element | [fire_element_067](nodes/fire_element_067.sym) |
| monsters | 68 | Gargoyle | [gargoyle_068](nodes/gargoyle_068.sym) |
| monsters | 69 | Neviros | [neviros_069](nodes/neviros_069.sym) |
| monsters | 70 | Ice Warrior | [ice_warrior_070](nodes/ice_warrior_070.sym) |
| monsters | 71 | Fire Warrior | [fire_warrior_071](nodes/fire_warrior_071.sym) |
| monsters | 72 | Thunder Sword | [thunder_sword_072](nodes/thunder_sword_072.sym) |
| monsters | 73 | Fake | [fake_073](nodes/fake_073.sym) |
| monsters | 74 | Water Element | [fire_element_067](nodes/fire_element_067.sym) |
| monsters | 75 | Wind Element | [fire_element_067](nodes/fire_element_067.sym) |
| monsters | 76 | Earth Element | [earth_element_076](nodes/earth_element_076.sym) |
| monsters | 77 | Hammer Knuckle | [hammer_knuckle_077](nodes/hammer_knuckle_077.sym) |
| monsters | 78 | Murder | [murder_078](nodes/murder_078.sym) |
| monsters | 79 | Perfect Murder | [perfect_murder_079](nodes/perfect_murder_079.sym) |
| monsters | 80 | Raybit | [raybit_080](nodes/raybit_080.sym) |
| monsters | 81 | Cybit | [raybit_080](nodes/raybit_080.sym) |
| monsters | 82 | Thief | [thief_082](nodes/thief_082.sym) |
| monsters | 83 | Rogue | [rogue_083](nodes/rogue_083.sym) |
| monsters | 84 | Soldier | [soldier_084](nodes/soldier_084.sym) |
| monsters | 85 | Duelist | [duelist_085](nodes/duelist_085.sym) |
| monsters | 86 | Warrior | [warrior_086](nodes/warrior_086.sym) |
| monsters | 87 | Heavy Armor | [warrior_086](nodes/warrior_086.sym) |
| monsters | 88 | Dragon Rider | [dragon_rider_088](nodes/dragon_rider_088.sym) |
| monsters | 89 | Archer | [archer_089](nodes/archer_089.sym) |
| monsters | 90 | Ranger | [ranger_090](nodes/ranger_090.sym) |
| monsters | 91 | Witch | [witch_091](nodes/witch_091.sym) |
| monsters | 92 | Sorceress | [sorceress_092](nodes/sorceress_092.sym) |
| monsters | 93 | Sorcerer | [sorcerer_093](nodes/sorcerer_093.sym) |
| monsters | 94 | Druid | [druid_094](nodes/druid_094.sym) |
| monsters | 95 | Ogre | [ogre_095](nodes/ogre_095.sym) |
| monsters | 96 | Beast Ogre | [beast_ogre_096](nodes/beast_ogre_096.sym) |
| monsters | 97 | Whip Master | [whip_master_097](nodes/whip_master_097.sym) |
| monsters | 98 | Bowman | [bowman_098](nodes/bowman_098.sym) |
| monsters | 99 | Spearman | [spearman_099](nodes/spearman_099.sym) |
| monsters | 100 | Foot Soldier | [foot_soldier_100](nodes/foot_soldier_100.sym) |
| monsters | 101 | Commander | [commander_101](nodes/commander_101.sym) |
| monsters | 102 | Cardinal Knight | [cardinal_knight_102](nodes/cardinal_knight_102.sym) |
| monsters | 103 | Commander Knight | [commander_knight_103](nodes/commander_knight_103.sym) |
| monsters | 104 | Evil Warrior | [evil_warrior_104](nodes/evil_warrior_104.sym) |
| monsters | 105 | Convict | [convict_105](nodes/convict_105.sym) |
| monsters | 106 | Evil Sorcerer | [evil_sorcerer_106](nodes/evil_sorcerer_106.sym) |
| monsters | 107 | Angel Spearman | [angel_spearman_107](nodes/angel_spearman_107.sym) |
| monsters | 108 | Angel Swordian | [angel_swordian_108](nodes/angel_swordian_108.sym) |
| monsters | 109 | Angel Commander | [angel_commander_109](nodes/angel_commander_109.sym) |
| monsters | 110 | Angel Archer | [angel_archer_110](nodes/angel_archer_110.sym) |
| monsters | 111 | Hawk | [hawk_111](nodes/hawk_111.sym) |
| monsters | 112 | Storm Claw | [hawk_111](nodes/hawk_111.sym) |
| monsters | 113 | Axe Beak | [axe_beak_113](nodes/axe_beak_113.sym) |
| monsters | 114 | Dodo | [dodo_114](nodes/dodo_114.sym) |
| monsters | 115 | Harpy | [harpy_115](nodes/harpy_115.sym) |
| monsters | 116 | Feather Magic | [harpy_115](nodes/harpy_115.sym) |
| monsters | 117 | Fire Bird | [fire_bird_117](nodes/fire_bird_117.sym) |
| monsters | 118 | Lightning Bird | [fire_bird_117](nodes/fire_bird_117.sym) |
| monsters | 119 | Penguinist | [penguinist_119](nodes/penguinist_119.sym) |
| monsters | 120 | Penguiner | [penguiner_120](nodes/penguiner_120.sym) |
| monsters | 121 | Black Bat | [black_bat_121](nodes/black_bat_121.sym) |
| monsters | 122 | Cockatrice | [cockatrice_122](nodes/cockatrice_122.sym) |
| monsters | 123 | Red Bat | [black_bat_121](nodes/black_bat_121.sym) |
| monsters | 124 | Giant Bee | [giant_bee_124](nodes/giant_bee_124.sym) |
| monsters | 125 | Killer Bee | [killer_bee_125](nodes/killer_bee_125.sym) |
| monsters | 126 | Scorpion | [scorpion_126](nodes/scorpion_126.sym) |
| monsters | 127 | Scarlet Needle | [scarlet_needle_127](nodes/scarlet_needle_127.sym) |
| monsters | 128 | Woods Worm | [woods_worm_128](nodes/woods_worm_128.sym) |
| monsters | 129 | Tropical Worm | [woods_worm_128](nodes/woods_worm_128.sym) |
| monsters | 130 | Sand Worm | [sand_worm_130](nodes/sand_worm_130.sym) |
| monsters | 131 | Sliver | [sand_worm_130](nodes/sand_worm_130.sym) |
| monsters | 132 | Mantis | [mantis_132](nodes/mantis_132.sym) |
| monsters | 133 | Red Mantis | [mantis_132](nodes/mantis_132.sym) |
| monsters | 134 | Spider | [spider_134](nodes/spider_134.sym) |
| monsters | 135 | Arachnid | [spider_134](nodes/spider_134.sym) |
| monsters | 136 | Giant Beetle | [giant_beetle_136](nodes/giant_beetle_136.sym) |
| monsters | 137 | Gold Beetle | [giant_beetle_136](nodes/giant_beetle_136.sym) |
| monsters | 138 | Grasshopper | [mantis_132](nodes/mantis_132.sym) |
| monsters | 139 | Ice Spider | [spider_134](nodes/spider_134.sym) |
| monsters | 140 | Deathseeker | [scorpion_126](nodes/scorpion_126.sym) |
| monsters | 141 | Starfish | [starfish_141](nodes/starfish_141.sym) |
| monsters | 142 | Super Star | [super_star_142](nodes/super_star_142.sym) |
| monsters | 143 | Tortoise | [tortoise_143](nodes/tortoise_143.sym) |
| monsters | 144 | Crush Tortoise | [crush_tortoise_144](nodes/crush_tortoise_144.sym) |
| monsters | 145 | Octoslime | [octoslime_145](nodes/octoslime_145.sym) |
| monsters | 146 | Kraaken | [kraaken_146](nodes/kraaken_146.sym) |
| monsters | 147 | Fish | [fish_147](nodes/fish_147.sym) |
| monsters | 148 | Seaspin | [fish_147](nodes/fish_147.sym) |
| monsters | 149 | Float Dragon | [float_dragon_149](nodes/float_dragon_149.sym) |
| monsters | 150 | Seahorse | [float_dragon_149](nodes/float_dragon_149.sym) |
| monsters | 151 | Jellyfish | [jellyfish_151](nodes/jellyfish_151.sym) |
| monsters | 152 | Sea Jelly | [sea_jelly_152](nodes/sea_jelly_152.sym) |
| monsters | 153 | Mermaid | [mermaid_153](nodes/mermaid_153.sym) |
| monsters | 154 | Evil Jelly | [sea_jelly_152](nodes/sea_jelly_152.sym) |
| monsters | 155 | Sea Dragon | [sea_dragon_155](nodes/sea_dragon_155.sym) |
| monsters | 156 | Sea Horror | [sea_horror_156](nodes/sea_horror_156.sym) |
| monsters | 157 | Slime | [slime_157](nodes/slime_157.sym) |
| monsters | 158 | Gold Slime | [gold_slime_158](nodes/gold_slime_158.sym) |
| monsters | 159 | Giant Leech | [giant_leech_159](nodes/giant_leech_159.sym) |
| monsters | 160 | Giant Slug | [giant_leech_159](nodes/giant_leech_159.sym) |
| monsters | 161 | Roller Snail | [roller_snail_161](nodes/roller_snail_161.sym) |
| monsters | 162 | Giant Snail | [roller_snail_161](nodes/roller_snail_161.sym) |
| monsters | 163 | Green Roper | [green_roper_163](nodes/green_roper_163.sym) |
| monsters | 164 | Red Roper | [red_roper_164](nodes/red_roper_164.sym) |
| monsters | 165 | Bacura | [bacura_165](nodes/bacura_165.sym) |
| monsters | 166 | Cutlass | [cutlass_166](nodes/cutlass_166.sym) |
| monsters | 167 | Cave Worm | [cave_worm_167](nodes/cave_worm_167.sym) |
| monsters | 168 | Man-eater | [man_eater_168](nodes/man_eater_168.sym) |
| monsters | 169 | Sheldra | [cutlass_166](nodes/cutlass_166.sym) |
| monsters | 170 | Spiked Snail | [roller_snail_161](nodes/roller_snail_161.sym) |
| monsters | 171 | Wyvern | [wyvern_171](nodes/wyvern_171.sym) |
| monsters | 172 | Drake | [wyvern_171](nodes/wyvern_171.sym) |
| monsters | 173 | Dragon | [dragon_173](nodes/dragon_173.sym) |
| monsters | 174 | Gold Dragon | [dragon_173](nodes/dragon_173.sym) |
| monsters | 175 | Dark Dragon | [dark_dragon_175](nodes/dark_dragon_175.sym) |
| monsters | 176 | Dragon Knight | [dragon_knight_176](nodes/dragon_knight_176.sym) |
| monsters | 177 | Velocidragon | [velocidragon_177](nodes/velocidragon_177.sym) |
| monsters | 178 | Exbelua | [exbelua_178](nodes/exbelua_178.sym) |
| monsters | 179 | Windmaster | [windmaster_179](nodes/windmaster_179.sym) |
| monsters | 180 | Ktugach | [ktugach_180](nodes/ktugach_180.sym) |
| monsters | 181 | Ktugachling | [ktugachling_181](nodes/ktugachling_181.sym) |
| monsters | 182 | Adulocia | [adulocia_182](nodes/adulocia_182.sym) |
| monsters | 183 | Amphitra | [amphitra_183](nodes/amphitra_183.sym) |
| monsters | 184 | Iapyx | [iapyx_184](nodes/iapyx_184.sym) |
| monsters | 185 | Iubaris | [iubaris_185](nodes/iubaris_185.sym) |
| monsters | 186 | Kilia | [kilia_186](nodes/kilia_186.sym) |
| monsters | 187 | Winged Dragon | [winged_dragon_187](nodes/winged_dragon_187.sym) |
| monsters | 188 | Baby Dragon | [baby_dragon_188](nodes/baby_dragon_188.sym) |
| monsters | 189 | Guardian:Wind | [guardian_wind_189](nodes/guardian_wind_189.sym) |
| monsters | 190 | Guardian:Lightning | [guardian_lightning_190](nodes/guardian_lightning_190.sym) |
| monsters | 191 | Sword Dancer | [sword_dancer_191](nodes/sword_dancer_191.sym) |
| monsters | 192 | Fenrir | [fenrir_192](nodes/fenrir_192.sym) |
| monsters | 193 | Idun | [idun_193](nodes/idun_193.sym) |
| monsters | 194 | Rodyle | [rodyle_194](nodes/rodyle_194.sym) |
| monsters | 195 | Undine | [undine_195](nodes/undine_195.sym) |
| monsters | 196 | Gnome | [gnome_196](nodes/gnome_196.sym) |
| monsters | 197 | Efreet | [efreet_197](nodes/efreet_197.sym) |
| monsters | 198 | Volt | [volt_198](nodes/volt_198.sym) |
| monsters | 199 | Celsius | [celsius_199](nodes/celsius_199.sym) |
| monsters | 200 | Luna | [luna_200](nodes/luna_200.sym) |
| monsters | 201 | Aska | [aska_201](nodes/aska_201.sym) |
| monsters | 202 | Shadow | [shadow_202](nodes/shadow_202.sym) |
| monsters | 203 | Maxwell | [maxwell_203](nodes/maxwell_203.sym) |
| monsters | 204 | Origin | [origin_204](nodes/origin_204.sym) |
| monsters | 205 | Sephie | [sephie_205](nodes/sephie_205.sym) |
| monsters | 206 | Yutis | [yutis_206](nodes/yutis_206.sym) |
| monsters | 207 | Fairess | [fairess_207](nodes/fairess_207.sym) |
| monsters | 208 | The Fugitive | [the_fugitive_208](nodes/the_fugitive_208.sym) |
| monsters | 209 | The Neglected | [the_fugitive_208](nodes/the_fugitive_208.sym) |
| monsters | 210 | The Judged | [the_fugitive_208](nodes/the_fugitive_208.sym) |
| monsters | 211 | Defense System | [defense_system_211](nodes/defense_system_211.sym) |
| monsters | 212 | Orbit | [raybit_080](nodes/raybit_080.sym) |
| monsters | 213 | Guard Arm | [guard_arm_213](nodes/guard_arm_213.sym) |
| monsters | 214 | Auto Repair Unit | [auto_repair_unit_214](nodes/auto_repair_unit_214.sym) |
| monsters | 215 | Kratos Aurion | [kratos_aurion_215](nodes/kratos_aurion_215.sym) |
| monsters | 216 | Magnius | [magnius_216](nodes/magnius_216.sym) |
| monsters | 217 | Kvar | [kvar_217](nodes/kvar_217.sym) |
| monsters | 218 | Energy Stone | [energy_stone_218](nodes/energy_stone_218.sym) |
| monsters | 219 | Vidarr | [vidarr_219](nodes/vidarr_219.sym) |
| monsters | 220 | Forcystus | [forcystus_220](nodes/forcystus_220.sym) |
| monsters | 221 | Exbone | [exbone_221](nodes/exbone_221.sym) |
| monsters | 222 | Pronyma | [pronyma_222](nodes/pronyma_222.sym) |
| monsters | 223 | Pronyma | [pronyma_222](nodes/pronyma_222.sym) |
| monsters | 224 | Clumsy Assassin | [clumsy_assassin_224](nodes/clumsy_assassin_224.sym) |
| monsters | 225 | Resolute Assassin | [clumsy_assassin_224](nodes/clumsy_assassin_224.sym) |
| monsters | 226 | Convict | [convict_226](nodes/convict_226.sym) |
| monsters | 227 | Kuchinawa | [kuchinawa_227](nodes/kuchinawa_227.sym) |
| monsters | 228 | Botta | [botta_228](nodes/botta_228.sym) |
| monsters | 229 | Botta | [botta_228](nodes/botta_228.sym) |
| monsters | 230 | Seles | [seles_230](nodes/seles_230.sym) |
| monsters | 231 | Garr | [garr_231](nodes/garr_231.sym) |
| monsters | 232 | Farah Oersted | [farah_oersted_232](nodes/farah_oersted_232.sym) |
| monsters | 233 | Meredy | [meredy_233](nodes/meredy_233.sym) |
| monsters | 234 | Abyssion | [abyssion_234](nodes/abyssion_234.sym) |
| monsters | 235 | Zelos Wilder | [zelos_wilder_235](nodes/zelos_wilder_235.sym) |
| monsters | 236 | Yggdrasill | [yggdrasill_236](nodes/yggdrasill_236.sym) |
| monsters | 237 | Yggdrasill | [yggdrasill_236](nodes/yggdrasill_236.sym) |
| monsters | 238 | Yggdrasill | [yggdrasill_236](nodes/yggdrasill_236.sym) |
| monsters | 239 | Mithos | [mithos_239](nodes/mithos_239.sym) |
| monsters | 240 | Mithos | [mithos_240](nodes/mithos_240.sym) |
| monsters | 241 | Kratos Aurion | [kratos_aurion_241](nodes/kratos_aurion_241.sym) |
| monsters | 242 | Kratos Aurion | [kratos_aurion_215](nodes/kratos_aurion_215.sym) |
| monsters | 243 | Yuan | [yuan_243](nodes/yuan_243.sym) |
| monsters | 244 | Remiel | [remiel_244](nodes/remiel_244.sym) |
| monsters | 245 | Gatekeeper | [gatekeeper_245](nodes/gatekeeper_245.sym) |
| monsters | 246 | Plantix | [plantix_246](nodes/plantix_246.sym) |
| monsters | 247 | Dark Spear | [angel_spearman_107](nodes/angel_spearman_107.sym) |
| monsters | 248 | Dark Sword | [dark_sword_248](nodes/dark_sword_248.sym) |
| monsters | 249 | Dark Commander | [angel_commander_109](nodes/angel_commander_109.sym) |
| monsters | 250 | Dark Archer | [angel_archer_110](nodes/angel_archer_110.sym) |
| figurines | 0 | Lloyd Irving | [lloyd_irving_000](nodes/lloyd_irving_000.sym) |
| figurines | 1 | Colette Brunel | [colette_brunel_001](nodes/colette_brunel_001.sym) |
| figurines | 2 | Genis Sage | [genis_sage_002](nodes/genis_sage_002.sym) |
| figurines | 3 | Raine Sage | [raine_sage_003](nodes/raine_sage_003.sym) |
| figurines | 4 | Sheena Fujibayashi | [clumsy_assassin_224](nodes/clumsy_assassin_224.sym) |
| figurines | 5 | Zelos Wilder | [zelos_wilder_235](nodes/zelos_wilder_235.sym) |
| figurines | 6 | Presea Combatir | [presea_combatir_006](nodes/presea_combatir_006.sym) |
| figurines | 7 | Regal Bryant | [convict_226](nodes/convict_226.sym) |
| figurines | 8 | Kratos Aurion | [kratos_aurion_215](nodes/kratos_aurion_215.sym) |
| figurines | 9 | Noishe | [noishe_009](nodes/noishe_009.sym) |
| figurines | 10 | Lloyd (Formal Dress) | [lloyd_irving_000](nodes/lloyd_irving_000.sym) |
| figurines | 11 | Colette (Formal Dress) | [colette_formal_dress_011](nodes/colette_formal_dress_011.sym) |
| figurines | 12 | Genis (Formal Dress) | [genis_sage_002](nodes/genis_sage_002.sym) |
| figurines | 13 | Raine (Formal Dress) | [raine_formal_dress_013](nodes/raine_formal_dress_013.sym) |
| figurines | 14 | Sheena (Formal Dress) | [sheena_formal_dress_014](nodes/sheena_formal_dress_014.sym) |
| figurines | 15 | Zelos (Formal Dress) | [zelos_wilder_235](nodes/zelos_wilder_235.sym) |
| figurines | 16 | Presea (Formal Dress) | [presea_formal_dress_016](nodes/presea_formal_dress_016.sym) |
| figurines | 17 | Regal (Formal Dress) | [regal_formal_dress_017](nodes/regal_formal_dress_017.sym) |
| figurines | 18 | Lloyd (Pirate) | [lloyd_pirate_018](nodes/lloyd_pirate_018.sym) |
| figurines | 19 | Colette (Maid) | [colette_maid_019](nodes/colette_maid_019.sym) |
| figurines | 20 | Genis (Katz) | [genis_katz_020](nodes/genis_katz_020.sym) |
| figurines | 21 | Raine (Maiden) | [raine_maiden_021](nodes/raine_maiden_021.sym) |
| figurines | 22 | Sheena (Next Chief) | [sheena_next_chief_022](nodes/sheena_next_chief_022.sym) |
| figurines | 23 | Zelos (Masked) | [zelos_masked_023](nodes/zelos_masked_023.sym) |
| figurines | 24 | Presea (Klonoa) | [presea_klonoa_024](nodes/presea_klonoa_024.sym) |
| figurines | 25 | Regal (Chef) | [regal_formal_dress_017](nodes/regal_formal_dress_017.sym) |
| figurines | 26 | Kratos (Cruxis) | [kratos_aurion_241](nodes/kratos_aurion_241.sym) |
| figurines | 27 | Lloyd (Swimsuit) | [lloyd_swimsuit_027](nodes/lloyd_swimsuit_027.sym) |
| figurines | 28 | Colette (Swimsuit) | [colette_formal_dress_011](nodes/colette_formal_dress_011.sym) |
| figurines | 29 | Genis (Swimsuit) | [genis_swimsuit_029](nodes/genis_swimsuit_029.sym) |
| figurines | 30 | Raine (Swimsuit) | [raine_swimsuit_030](nodes/raine_swimsuit_030.sym) |
| figurines | 31 | Sheena (Swimsuit) | [sheena_next_chief_022](nodes/sheena_next_chief_022.sym) |
| figurines | 32 | Zelos (Swimsuit) | [zelos_wilder_235](nodes/zelos_wilder_235.sym) |
| figurines | 33 | Presea (Swimsuit) | [presea_swimsuit_033](nodes/presea_swimsuit_033.sym) |
| figurines | 34 | Regal (Swimsuit) | [convict_226](nodes/convict_226.sym) |
| figurines | 35 | Yggdrasill | [yggdrasill_236](nodes/yggdrasill_236.sym) |
| figurines | 36 | Mithos | [mithos_036](nodes/mithos_036.sym) |
| figurines | 37 | Martel | [martel_037](nodes/martel_037.sym) |
| figurines | 38 | Yuan | [yuan_038](nodes/yuan_038.sym) |
| figurines | 39 | Botta | [botta_039](nodes/botta_039.sym) |
| figurines | 40 | Altessa | [altessa_040](nodes/altessa_040.sym) |
| figurines | 41 | Tabatha | [tabatha_041](nodes/tabatha_041.sym) |
| figurines | 42 | Remiel | [remiel_244](nodes/remiel_244.sym) |
| figurines | 43 | Magnius | [magnius_216](nodes/magnius_216.sym) |
| figurines | 44 | Kvar | [kvar_217](nodes/kvar_217.sym) |
| figurines | 45 | Rodyle | [rodyle_045](nodes/rodyle_045.sym) |
| figurines | 46 | Forcystus | [forcystus_220](nodes/forcystus_220.sym) |
| figurines | 47 | Pronyma | [pronyma_222](nodes/pronyma_222.sym) |
| figurines | 48 | Dirk | [dirk_048](nodes/dirk_048.sym) |
| figurines | 49 | Phaidra Brunel | [phaidra_brunel_049](nodes/phaidra_brunel_049.sym) |
| figurines | 50 | Frank Brunel | [frank_brunel_050](nodes/frank_brunel_050.sym) |
| figurines | 51 | Sebastian | [sebastian_051](nodes/sebastian_051.sym) |
| figurines | 52 | Seles | [seles_052](nodes/seles_052.sym) |
| figurines | 53 | Tokunaga | [tokunaga_053](nodes/tokunaga_053.sym) |
| figurines | 54 | Virginia | [virginia_054](nodes/virginia_054.sym) |
| figurines | 55 | Chief Igaguri | [chief_igaguri_055](nodes/chief_igaguri_055.sym) |
| figurines | 56 | Tiga | [tiga_056](nodes/tiga_056.sym) |
| figurines | 57 | Orochi | [orochi_057](nodes/orochi_057.sym) |
| figurines | 58 | Kuchinawa | [kuchinawa_058](nodes/kuchinawa_058.sym) |
| figurines | 59 | George | [george_059](nodes/george_059.sym) |
| figurines | 60 | Alicia Combatir | [alicia_combatir_060](nodes/alicia_combatir_060.sym) |
| figurines | 61 | Regal (Young) | [regal_young_061](nodes/regal_young_061.sym) |
| figurines | 62 | Vharley | [vharley_062](nodes/vharley_062.sym) |
| figurines | 63 | Abyssion | [abyssion_063](nodes/abyssion_063.sym) |
| figurines | 64 | Mayor of Iselia | [mayor_of_iselia_064](nodes/mayor_of_iselia_064.sym) |
| figurines | 65 | Marble | [marble_065](nodes/marble_065.sym) |
| figurines | 66 | Chocolat | [chocolat_066](nodes/chocolat_066.sym) |
| figurines | 67 | Cacao | [cacao_067](nodes/cacao_067.sym) |
| figurines | 68 | Dorr | [dorr_068](nodes/dorr_068.sym) |
| figurines | 69 | Kilia | [kilia_069](nodes/kilia_069.sym) |
| figurines | 70 | Clara | [clara_070](nodes/clara_070.sym) |
| figurines | 71 | Neil | [neil_071](nodes/neil_071.sym) |
| figurines | 72 | King of Tethe'alla | [king_of_tethe_alla_072](nodes/king_of_tethe_alla_072.sym) |
| figurines | 73 | Hilda | [hilda_073](nodes/hilda_073.sym) |
| figurines | 74 | Pope | [pope_074](nodes/pope_074.sym) |
| figurines | 75 | Kate | [kate_075](nodes/kate_075.sym) |
| figurines | 76 | Undine | [undine_076](nodes/undine_076.sym) |
| figurines | 77 | Sylph Sephie | [sephie_205](nodes/sephie_205.sym) |
| figurines | 78 | Sylph Yutis | [yutis_206](nodes/yutis_206.sym) |
| figurines | 79 | Sylph Fairess | [fairess_207](nodes/fairess_207.sym) |
| figurines | 80 | Efreet | [efreet_197](nodes/efreet_197.sym) |
| figurines | 81 | Gnome | [gnome_081](nodes/gnome_081.sym) |
| figurines | 82 | Volt | [volt_082](nodes/volt_082.sym) |
| figurines | 83 | Celsius | [celsius_199](nodes/celsius_199.sym) |
| figurines | 84 | Luna | [luna_200](nodes/luna_200.sym) |
| figurines | 85 | Aska | [aska_085](nodes/aska_085.sym) |
| figurines | 86 | Shadow | [shadow_202](nodes/shadow_202.sym) |
| figurines | 87 | Maxwell | [maxwell_087](nodes/maxwell_087.sym) |
| figurines | 88 | Origin | [origin_088](nodes/origin_088.sym) |
| figurines | 89 | Corrine | [corrine_089](nodes/corrine_089.sym) |
| figurines | 90 | Verius | [verius_090](nodes/verius_090.sym) |
| figurines | 91 | Lloyd's Imposter | [lloyd_s_imposter_091](nodes/lloyd_s_imposter_091.sym) |
| figurines | 92 | Colette's Imposter | [colette_s_imposter_092](nodes/colette_s_imposter_092.sym) |
| figurines | 93 | Genis' Imposter | [genis_imposter_093](nodes/genis_imposter_093.sym) |
| figurines | 94 | Raine's Imposter | [raine_s_imposter_094](nodes/raine_s_imposter_094.sym) |
| figurines | 95 | Nova | [nova_095](nodes/nova_095.sym) |
| figurines | 96 | Sarah | [sarah_096](nodes/sarah_096.sym) |
| figurines | 97 | Alduin | [alduin_097](nodes/alduin_097.sym) |
| figurines | 98 | May | [may_098](nodes/may_098.sym) |
| figurines | 99 | Max | [max_099](nodes/max_099.sym) |
| figurines | 100 | Lyla | [lyla_100](nodes/lyla_100.sym) |
| figurines | 101 | Aifread | [aifread_101](nodes/aifread_101.sym) |
| figurines | 102 | Koton | [koton_102](nodes/koton_102.sym) |
| figurines | 103 | Harley | [harley_103](nodes/harley_103.sym) |
| figurines | 104 | Linar | [linar_104](nodes/linar_104.sym) |
| figurines | 105 | Aisha | [aisha_105](nodes/aisha_105.sym) |
| figurines | 106 | Sophia | [sophia_106](nodes/sophia_106.sym) |
| figurines | 107 | Pietro | [pietro_107](nodes/pietro_107.sym) |
| figurines | 108 | Elven Elder | [elven_elder_108](nodes/elven_elder_108.sym) |
| figurines | 109 | Storyteller | [storyteller_109](nodes/storyteller_109.sym) |
| figurines | 110 | Gnomelette | [gnomelette_110](nodes/gnomelette_110.sym) |
| figurines | 111 | Unicorn | [unicorn_111](nodes/unicorn_111.sym) |
| figurines | 112 | Wonder Chef | [wonder_chef_112](nodes/wonder_chef_112.sym) |
| figurines | 113 | Dark Chef | [dark_chef_113](nodes/dark_chef_113.sym) |
| figurines | 114 | Alicia (Monster) | [alicia_monster_114](nodes/alicia_monster_114.sym) |
| figurines | 115 | Clara (Monster) | [clara_monster_115](nodes/clara_monster_115.sym) |
| figurines | 116 | Raine (Desian) | [raine_desian_116](nodes/raine_desian_116.sym) |
| figurines | 117 | Sheena (Desian) | [sheena_desian_117](nodes/sheena_desian_117.sym) |
| figurines | 118 | Pastor Marche | [pastor_marche_118](nodes/pastor_marche_118.sym) |
| figurines | 119 | Candy | [candy_119](nodes/candy_119.sym) |
| figurines | 120 | Mayor of Asgard | [mayor_of_asgard_120](nodes/mayor_of_asgard_120.sym) |
| figurines | 121 | New Mayor of Luin | [nova_095](nodes/nova_095.sym) |
| figurines | 122 | New Mayor's Daughter | [new_mayor_s_daughter_122](nodes/new_mayor_s_daughter_122.sym) |
| figurines | 123 | Doctor of Flanoir | [doctor_of_flanoir_123](nodes/doctor_of_flanoir_123.sym) |
| figurines | 124 | Elder of Exire | [elder_of_exire_124](nodes/elder_of_exire_124.sym) |
| figurines | 125 | High Pastor Auguste | [pastor_marche_118](nodes/pastor_marche_118.sym) |
| figurines | 126 | Mighty | [mighty_126](nodes/mighty_126.sym) |
| figurines | 127 | Holess | [holess_127](nodes/holess_127.sym) |
| figurines | 128 | Janet | [janet_128](nodes/janet_128.sym) |
| figurines | 129 | Levin | [levin_129](nodes/levin_129.sym) |
| figurines | 130 | Vice | [vice_130](nodes/vice_130.sym) |
| figurines | 131 | Noah | [noah_131](nodes/noah_131.sym) |
| figurines | 132 | Grace | [grace_132](nodes/grace_132.sym) |
| figurines | 133 | Joshua | [joshua_133](nodes/joshua_133.sym) |
| figurines | 134 | Rosa | [rosa_134](nodes/rosa_134.sym) |
| figurines | 135 | Norton | [norton_135](nodes/norton_135.sym) |
| figurines | 136 | Ralph | [ralph_136](nodes/ralph_136.sym) |
| figurines | 137 | Wells | [wells_137](nodes/wells_137.sym) |
| figurines | 138 | Mother of Four | [mother_of_four_138](nodes/mother_of_four_138.sym) |
| figurines | 139 | Beth | [beth_139](nodes/beth_139.sym) |
| figurines | 140 | Diana | [beth_139](nodes/beth_139.sym) |
| figurines | 141 | Mary | [beth_139](nodes/beth_139.sym) |
| figurines | 142 | Jo | [jo_142](nodes/jo_142.sym) |
| figurines | 143 | Crawly | [storyteller_109](nodes/storyteller_109.sym) |
| figurines | 144 | Ricardo | [ricardo_144](nodes/ricardo_144.sym) |
| figurines | 145 | Aaron | [aaron_145](nodes/aaron_145.sym) |
| figurines | 146 | Desian Male | [desian_male_146](nodes/desian_male_146.sym) |
| figurines | 147 | Desian Ranger | [desian_ranger_147](nodes/desian_ranger_147.sym) |
| figurines | 148 | Desian Mage | [desian_mage_148](nodes/desian_mage_148.sym) |
| figurines | 149 | Desian Female | [desian_female_149](nodes/desian_female_149.sym) |
| figurines | 150 | Renegade | [renegade_150](nodes/renegade_150.sym) |
| figurines | 151 | Militia (Iselia) | [militia_iselia_151](nodes/militia_iselia_151.sym) |
| figurines | 152 | Farmer (Iselia) | [farmer_iselia_152](nodes/farmer_iselia_152.sym) |
| figurines | 153 | Pastor (Iselia) | [pastor_iselia_153](nodes/pastor_iselia_153.sym) |
| figurines | 154 | Ranch Prisoner 1 | [ranch_prisoner_1_154](nodes/ranch_prisoner_1_154.sym) |
| figurines | 155 | Ranch Prisoner 2 | [ranch_prisoner_2_155](nodes/ranch_prisoner_2_155.sym) |
| figurines | 156 | Ranch Prisoner 3 | [ranch_prisoner_3_156](nodes/ranch_prisoner_3_156.sym) |
| figurines | 157 | Boy (Triet) | [boy_triet_157](nodes/boy_triet_157.sym) |
| figurines | 158 | Girl (Triet) | [girl_triet_158](nodes/girl_triet_158.sym) |
| figurines | 159 | Man (Triet) | [man_triet_159](nodes/man_triet_159.sym) |
| figurines | 160 | Woman (Triet) | [woman_triet_160](nodes/woman_triet_160.sym) |
| figurines | 161 | Fisherman (Izoold) | [fisherman_izoold_161](nodes/fisherman_izoold_161.sym) |
| figurines | 162 | Soldier (Palmacosta) | [soldier_palmacosta_162](nodes/soldier_palmacosta_162.sym) |
| figurines | 163 | Receptionist | [candy_119](nodes/candy_119.sym) |
| figurines | 164 | Tour Guide | [tour_guide_164](nodes/tour_guide_164.sym) |
| figurines | 165 | University Dean | [university_dean_165](nodes/university_dean_165.sym) |
| figurines | 166 | University Student 1 | [mighty_126](nodes/mighty_126.sym) |
| figurines | 167 | University Student 2 | [university_student_2_167](nodes/university_student_2_167.sym) |
| figurines | 168 | University Scholar | [university_scholar_168](nodes/university_scholar_168.sym) |
| figurines | 169 | Steamship Captain | [steamship_captain_169](nodes/steamship_captain_169.sym) |
| figurines | 170 | Steamship Crewman | [steamship_crewman_170](nodes/steamship_crewman_170.sym) |
| figurines | 171 | Adventurer Katz | [adventurer_katz_171](nodes/adventurer_katz_171.sym) |
| figurines | 172 | Katz | [adventurer_katz_171](nodes/adventurer_katz_171.sym) |
| figurines | 173 | Junior Katz | [junior_katz_173](nodes/junior_katz_173.sym) |
| figurines | 174 | Businessman Katz | [adventurer_katz_171](nodes/adventurer_katz_171.sym) |
| figurines | 175 | Elder Katz | [elder_katz_175](nodes/elder_katz_175.sym) |
| figurines | 176 | Boy (Sylvarant) | [boy_triet_157](nodes/boy_triet_157.sym) |
| figurines | 177 | Girl (Sylvarant) | [girl_sylvarant_177](nodes/girl_sylvarant_177.sym) |
| figurines | 178 | Man (Sylvarant) | [man_sylvarant_178](nodes/man_sylvarant_178.sym) |
| figurines | 179 | Woman 1 (Sylvarant) | [woman_1_sylvarant_179](nodes/woman_1_sylvarant_179.sym) |
| figurines | 180 | Woman 2 (Sylvarant) | [woman_1_sylvarant_179](nodes/woman_1_sylvarant_179.sym) |
| figurines | 181 | Man 2 (Sylvarant) | [man_2_sylvarant_181](nodes/man_2_sylvarant_181.sym) |
| figurines | 182 | Man 3 (Sylvarant) | [man_2_sylvarant_181](nodes/man_2_sylvarant_181.sym) |
| figurines | 183 | Woman 3 (Sylvarant) | [woman_3_sylvarant_183](nodes/woman_3_sylvarant_183.sym) |
| figurines | 184 | Old Man (Sylvarant) | [old_man_sylvarant_184](nodes/old_man_sylvarant_184.sym) |
| figurines | 185 | Old Woman (Sylvarant) | [ranch_prisoner_3_156](nodes/ranch_prisoner_3_156.sym) |
| figurines | 186 | Traveler (Sylvarant) | [traveler_sylvarant_186](nodes/traveler_sylvarant_186.sym) |
| figurines | 187 | Peddler (Sylvarant) | [peddler_sylvarant_187](nodes/peddler_sylvarant_187.sym) |
| figurines | 188 | Chef (Sylvarant) | [chef_sylvarant_188](nodes/chef_sylvarant_188.sym) |
| figurines | 189 | Doctor (Sylvarant) | [doctor_sylvarant_189](nodes/doctor_sylvarant_189.sym) |
| figurines | 190 | Maid (Sylvarant) | [maid_sylvarant_190](nodes/maid_sylvarant_190.sym) |
| figurines | 191 | Swordsman (Sylvarant) | [swordsman_sylvarant_191](nodes/swordsman_sylvarant_191.sym) |
| figurines | 192 | Mage (Sylvarant) | [mage_sylvarant_192](nodes/mage_sylvarant_192.sym) |
| figurines | 193 | Adventurer (Sylvarant) | [adventurer_sylvarant_193](nodes/adventurer_sylvarant_193.sym) |
| figurines | 194 | Pastor 1 (Sylvarant) | [pastor_iselia_153](nodes/pastor_iselia_153.sym) |
| figurines | 195 | Pastor 2 (Sylvarant) | [pastor_2_sylvarant_195](nodes/pastor_2_sylvarant_195.sym) |
| figurines | 196 | Minister (Tethe'alla) | [minister_tethe_alla_196](nodes/minister_tethe_alla_196.sym) |
| figurines | 197 | Commander (Tethe'alla) | [commander_tethe_alla_197](nodes/commander_tethe_alla_197.sym) |
| figurines | 198 | Soldier (Tethe'alla) | [soldier_tethe_alla_198](nodes/soldier_tethe_alla_198.sym) |
| figurines | 199 | Papal Commander | [papal_commander_199](nodes/papal_commander_199.sym) |
| figurines | 200 | Papal Knight | [papal_knight_200](nodes/papal_knight_200.sym) |
| figurines | 201 | Zelos' Groupie 1 | [zelos_groupie_1_201](nodes/zelos_groupie_1_201.sym) |
| figurines | 202 | Zelos' Groupie 2 | [zelos_groupie_1_201](nodes/zelos_groupie_1_201.sym) |
| figurines | 203 | Coliseum Receptionist | [coliseum_receptionist_203](nodes/coliseum_receptionist_203.sym) |
| figurines | 204 | Coliseum Announcer | [coliseum_announcer_204](nodes/coliseum_announcer_204.sym) |
| figurines | 205 | Nobleman 1 (Meltokio) | [nobleman_1_meltokio_205](nodes/nobleman_1_meltokio_205.sym) |
| figurines | 206 | Noblewoman 1 (Meltokio) | [janet_128](nodes/janet_128.sym) |
| figurines | 207 | Nobleman 2 (Meltokio) | [nobleman_2_meltokio_207](nodes/nobleman_2_meltokio_207.sym) |
| figurines | 208 | Noblewoman 2 (Meltokio) | [grace_132](nodes/grace_132.sym) |
| figurines | 209 | Peasant Boy (Meltokio) | [vice_130](nodes/vice_130.sym) |
| figurines | 210 | Peasant 1 (Meltokio) | [noah_131](nodes/noah_131.sym) |
| figurines | 211 | Peasant 2 (Meltokio) | [peasant_2_meltokio_211](nodes/peasant_2_meltokio_211.sym) |
| figurines | 212 | Prisoner Assassin | [prisoner_assassin_212](nodes/prisoner_assassin_212.sym) |
| figurines | 213 | Laboratory Director | [university_dean_165](nodes/university_dean_165.sym) |
| figurines | 214 | Laboratory Student 1 | [joshua_133](nodes/joshua_133.sym) |
| figurines | 215 | Laboratory Student 2 | [laboratory_student_2_215](nodes/laboratory_student_2_215.sym) |
| figurines | 216 | Laboratory Scholar 1 | [holess_127](nodes/holess_127.sym) |
| figurines | 217 | Laboratory Scholar 2 | [laboratory_scholar_2_217](nodes/laboratory_scholar_2_217.sym) |
| figurines | 218 | Laboratory Graduate 1 | [holess_127](nodes/holess_127.sym) |
| figurines | 219 | Laboratory Graduate 2 | [laboratory_scholar_2_217](nodes/laboratory_scholar_2_217.sym) |
| figurines | 220 | Half-Elf Scholar 1 | [half_elf_scholar_1_220](nodes/half_elf_scholar_1_220.sym) |
| figurines | 221 | Half-Elf Scholar 2 | [half_elf_scholar_2_221](nodes/half_elf_scholar_2_221.sym) |
| figurines | 222 | Laboratory Scholar 3 | [laboratory_scholar_3_222](nodes/laboratory_scholar_3_222.sym) |
| figurines | 223 | Laboratory Researcher | [norton_135](nodes/norton_135.sym) |
| figurines | 224 | Kage | [kage_224](nodes/kage_224.sym) |
| figurines | 225 | Boy (Mizuho) | [beth_139](nodes/beth_139.sym) |
| figurines | 226 | Girl (Mizuho) | [beth_139](nodes/beth_139.sym) |
| figurines | 227 | Man (Mizuho) | [man_mizuho_227](nodes/man_mizuho_227.sym) |
| figurines | 228 | Woman (Mizuho) | [woman_mizuho_228](nodes/woman_mizuho_228.sym) |
| figurines | 229 | Lumberjack (Ozette) | [lumberjack_ozette_229](nodes/lumberjack_ozette_229.sym) |
| figurines | 230 | Boy (Ozette) | [jo_142](nodes/jo_142.sym) |
| figurines | 231 | Girl (Ozette) | [beth_139](nodes/beth_139.sym) |
| figurines | 232 | Man 1 (Ozette) | [wells_137](nodes/wells_137.sym) |
| figurines | 233 | Woman 1 (Ozette) | [woman_1_ozette_233](nodes/woman_1_ozette_233.sym) |
| figurines | 234 | Man 2 (Ozette) | [ralph_136](nodes/ralph_136.sym) |
| figurines | 235 | Woman 2 (Ozette) | [mother_of_four_138](nodes/mother_of_four_138.sym) |
| figurines | 236 | Boy (Flanoir) | [boy_flanoir_236](nodes/boy_flanoir_236.sym) |
| figurines | 237 | Girl (Flanoir) | [boy_flanoir_236](nodes/boy_flanoir_236.sym) |
| figurines | 238 | Man 1 (Flanoir) | [man_1_flanoir_238](nodes/man_1_flanoir_238.sym) |
| figurines | 239 | Woman 1 (Flanoir) | [woman_1_flanoir_239](nodes/woman_1_flanoir_239.sym) |
| figurines | 240 | Man 2 (Flanoir) | [man_2_flanoir_240](nodes/man_2_flanoir_240.sym) |
| figurines | 241 | Woman 2 (Flanoir) | [mother_of_four_138](nodes/mother_of_four_138.sym) |
| figurines | 242 | Old Man (Flanoir) | [old_man_flanoir_242](nodes/old_man_flanoir_242.sym) |
| figurines | 243 | Company Employee | [company_employee_243](nodes/company_employee_243.sym) |
| figurines | 244 | Company Security | [company_security_244](nodes/company_security_244.sym) |
| figurines | 245 | Manager | [george_059](nodes/george_059.sym) |
| figurines | 246 | Bunny Girl | [bunny_girl_246](nodes/bunny_girl_246.sym) |
| figurines | 247 | Mascot Character | [dirk_048](nodes/dirk_048.sym) |
| figurines | 248 | Male Staff (Altamira) | [wells_137](nodes/wells_137.sym) |
| figurines | 249 | Boy (Altamira) | [jo_142](nodes/jo_142.sym) |
| figurines | 250 | Vacationing Man | [vacationing_man_250](nodes/vacationing_man_250.sym) |
| figurines | 251 | Vacationing Woman | [vacationing_woman_251](nodes/vacationing_woman_251.sym) |
| figurines | 252 | Elf Guard | [elf_guard_252](nodes/elf_guard_252.sym) |
| figurines | 253 | Elf Man 1 | [ricardo_144](nodes/ricardo_144.sym) |
| figurines | 254 | Elf Woman 1 | [elf_woman_1_254](nodes/elf_woman_1_254.sym) |
| figurines | 255 | Elf Man 2 | [elf_man_2_255](nodes/elf_man_2_255.sym) |
| figurines | 256 | Elf Woman 2 | [elf_woman_2_256](nodes/elf_woman_2_256.sym) |
| figurines | 257 | Half-Elf Boy | [half_elf_boy_257](nodes/half_elf_boy_257.sym) |
| figurines | 258 | Half-Elf Man 1 | [half_elf_man_1_258](nodes/half_elf_man_1_258.sym) |
| figurines | 259 | Half-Elf Woman 1 | [half_elf_woman_1_259](nodes/half_elf_woman_1_259.sym) |
| figurines | 260 | Half-Elf Man 2 | [noah_131](nodes/noah_131.sym) |
| figurines | 261 | Half-Elf Old Man | [storyteller_109](nodes/storyteller_109.sym) |
| figurines | 262 | Half-Elf Old Woman | [storyteller_109](nodes/storyteller_109.sym) |
| figurines | 263 | Male Angel | [male_angel_263](nodes/male_angel_263.sym) |
| figurines | 264 | Female Angel | [female_angel_264](nodes/female_angel_264.sym) |
| figurines | 265 | Boy (Tethe'alla) | [jo_142](nodes/jo_142.sym) |
| figurines | 266 | Girl (Tethe'alla) | [beth_139](nodes/beth_139.sym) |
| figurines | 267 | Man 1 (Tethe'alla) | [wells_137](nodes/wells_137.sym) |
| figurines | 268 | Man 2 (Tethe'alla) | [wells_137](nodes/wells_137.sym) |
| figurines | 269 | Woman 1 (Tethe'alla) | [woman_1_ozette_233](nodes/woman_1_ozette_233.sym) |
| figurines | 270 | Woman 2 (Tethe'alla) | [woman_1_ozette_233](nodes/woman_1_ozette_233.sym) |
| figurines | 271 | Man 3 (Tethe'alla) | [ralph_136](nodes/ralph_136.sym) |
| figurines | 272 | Woman 3 (Tethe'alla) | [mother_of_four_138](nodes/mother_of_four_138.sym) |
| figurines | 273 | Old Man (Tethe'alla) | [levin_129](nodes/levin_129.sym) |
| figurines | 274 | Old Woman (Tethe'alla) | [old_woman_tethe_alla_274](nodes/old_woman_tethe_alla_274.sym) |
| figurines | 275 | Traveler (Tethe'alla) | [traveler_sylvarant_186](nodes/traveler_sylvarant_186.sym) |
| figurines | 276 | Peddler (Tethe'alla) | [peddler_tethe_alla_276](nodes/peddler_tethe_alla_276.sym) |
| figurines | 277 | Chef (Tethe'alla) | [chef_sylvarant_188](nodes/chef_sylvarant_188.sym) |
| figurines | 278 | Nurse (Tethe'alla) | [rosa_134](nodes/rosa_134.sym) |
| figurines | 279 | Maid (Tethe'alla) | [rosa_134](nodes/rosa_134.sym) |
| figurines | 280 | Swordsman (Tethe'alla) | [swordsman_tethe_alla_280](nodes/swordsman_tethe_alla_280.sym) |
| figurines | 281 | Mage (Tethe'alla) | [mage_tethe_alla_281](nodes/mage_tethe_alla_281.sym) |
| figurines | 282 | Pastor 1 (Tethe'alla) | [pastor_iselia_153](nodes/pastor_iselia_153.sym) |
| figurines | 283 | Pastor 2 (Tethe'alla) | [pastor_2_sylvarant_195](nodes/pastor_2_sylvarant_195.sym) |
| figurines | 284 | Dog | [dog_284](nodes/dog_284.sym) |
| figurines | 285 | Cat | [cat_285](nodes/cat_285.sym) |
| figurines | 286 | Pigeon | [pigeon_286](nodes/pigeon_286.sym) |
| figurines | 287 | Bush Baby | [bush_baby_287](nodes/bush_baby_287.sym) |
