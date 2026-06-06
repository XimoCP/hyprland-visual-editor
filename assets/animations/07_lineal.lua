-- @Title: Linear
-- @Icon: chart-line
-- @Color: #2dd4bf
-- @Tag: FLAT
-- @Desc: Mathematical precision. Constant movement "Sci-Fi HUD" style.
-- Converted from 07_lineal.conf
-- =====================================

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("linear", { type = "bezier", points = { { 0.0, 0.0 }, { 1.0, 1.0 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 5, bezier = "linear", style = "slide" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 5, bezier = "linear", style = "slide" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 5, bezier = "linear", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 4, bezier = "linear" })
hl.animation({ leaf = "layers", enabled = true, speed = 4, bezier = "linear", style = "fade" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 5, bezier = "linear", style = "slide" })
