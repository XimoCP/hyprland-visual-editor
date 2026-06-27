-- @Title: Crystal
-- @Icon: droplet
-- @Color: #38bdf8
-- @Tag: VIBE
-- @Desc: Ethereal glide. Feeling of glass floating without friction.
-- Converted from 08_cristal.conf
-- =====================================

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("glass", { type = "bezier", points = { { 0.16, 1 }, { 0.3, 1 } } })
hl.curve("fade", { type = "bezier", points = { { 0.33, 1 }, { 0.68, 1 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 6, bezier = "glass", style = "slide" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 6, bezier = "glass", style = "popin 90%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 6, bezier = "glass", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 7, bezier = "fade" })
hl.animation({ leaf = "layers", enabled = true, speed = 6, bezier = "glass", style = "fade" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 6, bezier = "glass", style = "slidefade 10%" })
