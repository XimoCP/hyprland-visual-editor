-- @Title: Elastic Inertia
-- @Icon: adjustments
-- @Color: #60a5fa
-- @Tag: PHYSICS
-- @Desc: Organic movement. The windows gain momentum when leaving and bounce when entering.
-- Converted from 02_inercia_elastica.conf
-- =====================================

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("anticipate", { type = "bezier", points = { { 0.1, -0.1 }, { 0.1, 1.0 } } })
hl.curve("inertia", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.05 } } })
hl.curve("friction", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.0 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 5, bezier = "inertia", style = "slide" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 6, bezier = "anticipate", style = "slide" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 5, bezier = "friction", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 5, bezier = "friction" })
hl.animation({ leaf = "layers", enabled = true, speed = 5, bezier = "friction", style = "fade" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 6, bezier = "inertia", style = "slidefade 20%" })
