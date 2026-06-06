-- @Title: Retro Arcade
-- @Icon: device-gamepad-2
-- @Color: #f472b6
-- @Tag: 80s
-- @Desc: "Toon/Arcade" effect. The windows jump and bounce exaggeratedly.
-- Converted from 10_retro_arcade.conf
-- =====================================

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("jump", { type = "bezier", points = { { 0.5, -0.3 }, { 0.68, 1.4 } } })
hl.curve("squash", { type = "bezier", points = { { 0.5, -0.5 }, { 0.1, 1.0 } } })
hl.curve("rubber", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.05 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 5, bezier = "jump", style = "popin 80%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 4, bezier = "squash", style = "popin 80%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 4, bezier = "rubber", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 3, bezier = "default" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 5, bezier = "jump", style = "slide" })
