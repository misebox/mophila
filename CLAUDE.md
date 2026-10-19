DO NOT USE sed command (including `sed -i`)
DO NOT USE python to edit files or run one-off scripts. Edit files with the Edit / Write tools only.
Avoid any operation that triggers a permission prompt: shell variables, loops, wildcards or `$(…)` in paths, braces next to quotes, pipes into other commands, and any read or write outside this directory.
Put temporary files under `tmp/` in this directory, not in /tmp or the scratchpad.
