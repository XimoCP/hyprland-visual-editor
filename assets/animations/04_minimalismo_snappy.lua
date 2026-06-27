-- @Title: Snappy Minimalism
-- @Icon: bolt
-- @Color: #94a3b8
-- @Tag: SNAPPY
-- @Desc: Optimized for maximum speed. Only window and workspace management.
-- =====================================
hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("winIn", { type = "bezier", points = { { 0.07, 0.88 }, { 0.04, 0.99 } } })
hl.curve("menu_decel", { type = "bezier", points = { { 0.05, 0.82 }, { 0, 1 } } })
hl.curve("menu_accel", { type = "bezier", points = { { 0.38, 0 }, { 1, 1 } } })
hl.curve("md3_decel", { type = "bezier", points = { { 0.05, 0.8 }, { 0.1, 0.97 } } })
hl.curve("easeOutCirc", { type = "bezier", points = { { 0, 0.48 }, { 0.38, 1 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 3.2, bezier = "winIn", style = "slide" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 2.8, bezier = "easeOutCirc", style = "slide" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 3.0, bezier = "md3_decel", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 1.8, bezier = "md3_decel" })
hl.animation({ leaf = "layersIn", enabled = true, speed = 1.8, bezier = "menu_decel", style = "slide" })
hl.animation({ leaf = "layersOut", enabled = true, speed = 1.5, bezier = "menu_accel", style = "slide" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 4.0, bezier = "menu_decel", style = "slidefade 20%" })
hl.animation({ leaf = "specialWorkspace", enabled = true, speed = 2.3, bezier = "md3_decel", style = "slidefadevert 15%" })
