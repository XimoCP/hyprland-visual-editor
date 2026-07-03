-- @Title: Futuristic
-- @Icon: cpu
-- @Color: #22d3ee
-- @Tag: TECH
-- @Desc: Holographic interface. Digital precision, zero bounce and vertical data flow.
-- Converted from 11_futurista.conf
-- =====================================
hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("holo", { type = "bezier", points = { { 0.23, 1 }, { 0.32, 1 } } })
hl.curve("data", { type = "bezier", points = { { 0.16, 1 }, { 0.3, 1 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 5, bezier = "holo", style = "slide" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 4, bezier = "holo", style = "popin 100%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 5, bezier = "holo", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 5, bezier = "data" })
hl.animation({ leaf = "layers", enabled = true, speed = 5, bezier = "holo", style = "fade" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 6, bezier = "holo", style = "slidevert" })
hl.animation({ leaf = "specialWorkspace", enabled = true, speed = 6, bezier = "holo", style = "slidevert" })
