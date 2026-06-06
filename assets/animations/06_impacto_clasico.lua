-- @Title: Classic Impact
-- @Icon: flame
-- @Color: #fb7185
-- @Tag: FX
-- @Desc: Jelly effect. Elastic bounce on entry and anticipation on exit.
-- Converted from 06_impacto_clasico.conf
-- =====================================

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("elastic", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.05 } } })
hl.curve("retro", { type = "bezier", points = { { 0.6, -0.28 }, { 0.735, 0.045 } } })
hl.curve("smooth", { type = "bezier", points = { { 0.1, 1 }, { 0.1, 1 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 0.6, bezier = "elastic", style = "popin 80%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 0.5, bezier = "retro", style = "popin 80%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 0.5, bezier = "smooth", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 0.5, bezier = "smooth" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 0.6, bezier = "elastic", style = "slide" })
hl.animation({ leaf = "specialWorkspace", enabled = true, speed = 6, bezier = "elastic", style = "slidevert" })
