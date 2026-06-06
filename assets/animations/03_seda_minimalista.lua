-- @Title: Minimalist Silk
-- @Icon: circle-dot
-- @Color: #818cf8
-- @Tag: SOFT
-- @Desc: Absolute smoothness. No bouncing, just perfect macOS-style landings.
-- Converted from 03_seda_minimalista.conf
-- =====================================
hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("quart", { type = "bezier", points = { { 0.25, 1 }, { 0.5, 1 } } })
hl.curve("expo", { type = "bezier", points = { { 0.16, 1 }, { 0.3, 1 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 1, bezier = "quart", style = "popin 80%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 5, bezier = "quart", style = "popin 80%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 6, bezier = "quart", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 6, bezier = "quart" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 6, bezier = "expo", style = "slidefade 20%" })
