-- @Title: Modern Material
-- @Icon: layers-intersect
-- @Color: #c084fc
-- @Tag: MODERN
-- @Desc: Google Pixel aesthetic. Organic animations, subtle scales and haptic feedback.
-- Converted from 05_material_moderno.conf
-- =====================================

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("md3_decel", { type = "bezier", points = { { 0.05, 0.7 }, { 0.1, 1 } } })
hl.curve("md3_accel", { type = "bezier", points = { { 0.3, 0 }, { 0.8, 0.15 } } })
hl.curve("md3_standard", { type = "bezier", points = { { 0.2, 0 }, { 0, 1 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 4, bezier = "md3_decel", style = "popin 80%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 3, bezier = "md3_accel", style = "popin 80%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 3, bezier = "md3_standard", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 3, bezier = "md3_decel" })
hl.animation({ leaf = "layersIn", enabled = true, speed = 3, bezier = "md3_decel", style = "slide" })
hl.animation({ leaf = "layersOut", enabled = true, speed = 3, bezier = "md3_accel", style = "slide" })
hl.animation({ leaf = "fadeLayersIn", enabled = true, speed = 2, bezier = "md3_decel" })
hl.animation({ leaf = "fadeLayersOut", enabled = true, speed = 2, bezier = "md3_accel" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 5, bezier = "md3_decel", style = "slidefade 20%" })
hl.animation({ leaf = "specialWorkspace", enabled = true, speed = 3, bezier = "md3_decel", style = "slidevert" })
