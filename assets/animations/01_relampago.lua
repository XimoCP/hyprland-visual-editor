-- @Title: Relámpago
-- @Icon: bolt
-- @Color: #f87171
-- @Tag: FAST
-- @Desc: Máxima respuesta visual.
-- Converted from 01_relampago.conf
-- =====================================

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("lightning", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.05 } } })
hl.curve("smooth", { type = "bezier", points = { { 0.25, 1 }, { 0.5, 1 } } })
hl.curve("instant", { type = "bezier", points = { { 0.0, 1.0 }, { 1.0, 1.0 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 1, bezier = "lightning", style = "popin 80%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 1, bezier = "lightning", style = "popin 80%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 1, bezier = "instant", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 1, bezier = "smooth" })
hl.animation({ leaf = "layers", enabled = true, speed = 1, bezier = "lightning", style = "fade" })
hl.animation({ leaf = "layersIn", enabled = true, speed = 1, bezier = "lightning", style = "fade" })
hl.animation({ leaf = "layersOut", enabled = true, speed = 1, bezier = "lightning", style = "fade" })

hl.animation({ leaf = "workspaces", enabled = true, speed = 1, bezier = "lightning", style = "slidefade 20%" })
hl.animation({ leaf = "specialWorkspace", enabled = true, speed = 1, bezier = "lightning", style = "slidevert" })
