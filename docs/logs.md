# Log access

Settings provides a read-only viewer for RuneLite launch and client logs, official OSRS and RS3 launch logs, and Wine setup output. Refresh reloads the selected log. Copy copies the displayed output; Open folder opens its existing client folder in the system file manager. A launch error mentioning a log exposes a View log shortcut.

Reads are limited to the last 128 KiB. Missing and empty files have explicit messages. Partial first lines are discarded when truncating. HTML is never interpreted. Only typed, fixed log sources are exposed to the local launcher window; authentication pages have no command permissions. Lines containing common credential markers and saved session secrets are filtered from the viewer and copied text. Raw files opened through the file manager retain their original contents. Logs may still include account or character names.

Backend tests cover credential filtering, control characters, missing files and bounded reads with incomplete first lines. Interactive copy and folder opening still need acceptance on Windows and Linux.
