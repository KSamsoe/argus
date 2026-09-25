# argus-dialogue: spec trace

Every statement in the design document (v0.3) that constrains dialogue (§12 "Dialogue and localization", §4 data tables, §13 audio and timelines, §6 determinism), with where it lives and the test that pins it. Phase 3 item 4. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §12 Dialogue and localization

| Spec | Status | Where | Test |
|---|---|---|---|
| "Dialogue as a def table (§4) with a small node graph." | Done | `graph::schema`: the engine declares the `dialogue` def table (prefix `dialogue`, key `id`), so every project has one and `defs/dialogue.toml` is its rows. A row is a node, or with `from` a choice (it has a line) or a branch (it has none) of one; `next` goes on. `Graph::from_defs` reads the rows once, conditions and effects parsed, dangling `from` and `next` said. | `dialogue::a_conversation_shows_lines_offers_choices_and_makes_its_edits`, `dialogue::rows_that_do_not_read_are_said_and_a_missing_node_ends_the_talk` |
| "Lines, choices." | Done | A node shows its `line` said by its `speaker`, both string keys, so the conversation is in the world's language; its choices are the rows from it with a line whose condition holds, offered as buttons in the box, in file order. A node without choices has a button to go on (`dialogue.continue`). | `dialogue::a_conversation_shows_lines_offers_choices_and_makes_its_edits` |
| "Conditions on world state." | Done | `when`: comparisons of `[who:]Component.field` with a number, a bool, or words, joined by `and` (first) and `or`, `not` one; `who` is `listener` (the default), `speaker`, or an authored `@id`. A node whose `when` fails is passed over to its `next`; a choice whose `when` fails is not offered; the first branch whose `when` holds is taken. | `dialogue::a_second_meeting_branches_and_a_spent_choice_is_not_offered`; Hollow's `tests/hermit.luau` (the gem offered once found, until given) |
| "Effects as edits." | Done | `effects`: `path = value`, `path += n`, `path -= n`, `emit event`, separated by `;`, made when a node shows or a choice or branch is taken. A field's edit is a write of the world like any system's (declared as it happens, as a tween's target is, so `who_writes` stays true), typed by the field; an edit that cannot be made is a `dialogue.problem` event, not a fault. | `dialogue::a_conversation_shows_lines_offers_choices_and_makes_its_edits` |
| "Runnable from Luau." | Done | A conversation is a `Talk` entity: a script spawns one with its start, speaker, listener, and player slot (`q:spawn({"Talk"})`). `dialogue.update` (sim stage, after the UI's input, unfrozen) plays it: the box's buttons pressed with the player's actions, or `Talk.pick` and `Talk.advance` set by a script, the protocol, or a test. Its state (node, line and speaker's name in the world's language, choices offered, steps) is in the component; it emits `dialogue.started`, `dialogue.line`, `dialogue.chose`, `dialogue.ended`, and despawns with its box when it ends. | `dialogue::the_box_is_driven_by_a_players_actions_and_speaks_the_worlds_language`; Hollow's hermit |
| "Inspectable through world.defs." | Done | The table is a def table: `world.defs {table: "dialogue"}` lists its rows, `where` filters them, and the index has it. Rows that do not read are listed in `script.errors` (phase `dialogue`) with their file and line. | `argus-daemon` `text::strings_are_listed_changed_in_a_transaction_and_taken_back` |
| The box is the UI tree's. | Done | A panel along the top of the canvas, under a HUD line (found in a picture of the French hermit: along the bottom it hid the hero and the hermit) (`panel` and `button` theme colours): the speaker's name, the line, a button a choice. Its texts are `UiText.key`s, so the UI draws them in the world's language and a language chosen mid-conversation shows at once; the semantic frame's `ui` section reads the box as it reads any menu. | Hollow's `tests/hermit.luau` ("the game is in French when the settings say so") |

## §13, §4, §6

| Spec | Status | Where | Test |
|---|---|---|---|
| §13: "dialogue ducks music by 6 dB over 200 ms" is a ducking rule. | Done | A row's `sound` plays when its line shows (`sound.play`, from the speaker); the sound's bus is the project's to say, and a ducking rule on a dialogue bus lowers the music. Hollow's `audio.toml` has the rule as §13 writes it. | Hollow's `tests/hermit.luau` (the music bus ducked while he speaks) |
| §13: timelines have dialogue-line tracks. | Deferred | Item 5: a timeline track will start a `Talk`. | |
| §4: dialogue lines are data tables. | Done | The graph, and every word of it in the string tables. | |
| §6: the same inputs give the same world. | Done | Everything a conversation is, is in its `Talk` and the box's entities; the graph is content. A seek or a load plays on from the node it was at. | Hollow's tests (replayed on failure like any) |

## Known limitations, stated so they are decisions

- A node offers at most 32 choices, and a walk passes at most 64 nodes without a line before it stops and says so (a loop of branches).
- Conditions have no parentheses: `and` binds before `or`.
- The box's look is the engine's (the theme's colours, a bottom panel); a game that wants its own draws the `Talk`'s fields in its own UI and sets `pick` and `advance`.
- A conversation does not stop the game; a game freezes it while a `Talk` exists if it wants to (Hollow's hermit does, as its pause menu does).
