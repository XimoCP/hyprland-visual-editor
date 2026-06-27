-- @Title: Elastic
-- @Icon: arrows-diagonal
-- @Color: #6366f1
-- @Tag: FLEX
-- @Desc: Rubber band physics. The windows reach, stretch and bounce vertically.
-- Converted from 14_elastico.conf
-- =====================================

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("rubber", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.15 } } })
hl.curve("snap", { type = "bezier", points = { { 0.1, 1 }, { 0, 1 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 5, bezier = "rubber", style = "slidevert" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 4, bezier = "snap", style = "slidevert" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 5, bezier = "rubber", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 4, bezier = "default" })
hl.animation({ leaf = "layers", enabled = true, speed = 4, bezier = "rubber", style = "slidevert" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 6, bezier = "rubber", style = "slidevert" })
