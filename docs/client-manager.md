# Shared client manager

Game IDs: `oldschool`, `runescape`. Client IDs: `runelite`, `officialOsrs`, `officialRs3`.

Settings retain existing preferences and default older settings to Old School/RuneLite. Old School's client choice survives switching to RuneScape. The settings API rejects a client belonging to another game.

The registry supplies availability and reasons to the UI and backend. Official OSRS is available on Windows x64 and Linux x64 with Wine installed. Official RS3 is implemented for Windows x64 and Linux x64, pending live acceptance. A disabled Play button is backed by a manager availability check; invoking the command directly cannot prepare an unavailable client.

The manager owns installation progress and tracks each process by captured account and character. It serializes preparation per client, blocks duplicate running/queued characters, releases reservations after failure, and reaps exited processes. Closing the launcher does not kill games. Adoption after launcher restart remains out of scope.

RuneLite is the first adapter. Its existing `clients/runelite` directory and installation records are retained. Downloads finish before renewing credentials for the captured account/character. Changing the selected account while queued cannot redirect the login handoff to another account. Browser authentication and storage implementation are unchanged.

The frontend offers game/client selectors, selected-client status, and availability feedback. Different characters can launch after the prior preparation finishes. Test actual concurrent gameplay on both platforms before release.

Validation covers old-settings migration, game/client compatibility, saved choices, unavailable adapters, duplicate reservations, retry after failure, independent process tracking, captured account lookup, and frontend selection/launch behavior. Live RuneLite acceptance and Windows CI are separate checks.
