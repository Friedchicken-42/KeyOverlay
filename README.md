
# KeyOverlay
 A simple key overlay for osu! streaming
 
Edit `config.toml` o create a new `*.toml` file to set your keymap
# [Download Link](https://github.com/Friedchicken-42/KeyOverlay/releases/tag/v2.2)
IF YOU ARE HAVING PROBLEMS WITH THE PROGRAM WHEN OSU! IS ON FULLSCREEN, TRY USING GAME CAPTURE INSTEAD OF WINDOW CAPTURE IN OBS!

On NixOS:
```sh
nix run github:Friedchicken-42/KeyOverlay
```

# config.toml properties
```toml
# Required
speed = 200.0   # Adjusts the speed at which the bars travel upwards.
size = 44.0     # Changes the size of the key.

# Optional
height = 700.0  # Sets the height of the window.
gap = 18.0      # Adjusts the space between the keys.
bg = "#000000"  # Sets the background color.
counter = false # Shows the counter beneath each key.

# List of keys to display, options: `{ color, width, text }`
[keys]
Z = { color = "#ffffff" } 
X = { color = "#00ffff" }
C = { color = "#ffffff" }
# Custom width, otherwise defaults to 1
Space = { color = "#ffff00", width = 2 }
M = { color = "#ffffff" }
# Custom text, otherwise uses the name
Comma = { color = "#00ffff", text = "," }
Period = { color = "#ffffff", text = "." }
```

# Improvements
- Automatically loads the most recent `*.toml` file
- Supports dropping config files in the window
- Reloads when the config is edited

# Preview
![](https://imgur.com/JJOrZUt.gif)
_~~yeah i'm not good at 7k but this should give a good idea of the features~~_
