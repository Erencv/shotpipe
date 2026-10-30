# pi skill: screenshot

Install into the machine where pi runs (the server):

```bash
mkdir -p ~/.pi/agent/skills/screenshot
# save the block below as ~/.pi/agent/skills/screenshot/SKILL.md
```

`SKILL.md`:

```markdown
---
name: screenshot
description: Attach the user's screenshots or pasted images from their Mac. Use when the user pastes a file path (especially /var/folders/... or any path that doesn't exist on this machine), mentions a screenshot, or says "look at my screen / this image / the UI".
---

# Get the user's screenshot

The user works on this machine over SSH from a macOS laptop running shotpipe.
Images sync automatically into ~/shots/ (screenshots, clipboard copies, and
pasted images alike), usually within ~2 seconds.

## If the user pasted a path

When a message contains a file path, **first check if it exists locally**. If
it does not (typically a Mac path like /var/folders/.../T/clipboard-*.png
produced by pasting an image into the terminal), resolve it:

    shotpipe resolve <pasted-path>

This prints the synced local path (it waits for the sync to land, up to ~8s).
Use that path in place of the pasted one.

## Otherwise

- `~/shots/latest.png` — newest synced image; `ls -t ~/shots/` for older ones.
- `shotpipe in` — saves the image currently on the laptop's clipboard to
  /tmp/clip-*.png and prints the path.

Finally, use the `read` tool on the path to attach the image, then answer.
```

Requires a vision-capable model (`shotpipe resolve` only moves files; the
model must accept image input to actually see them).
