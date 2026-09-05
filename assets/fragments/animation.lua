-- @Title: Rebound
-- @Icon: trending-up
-- @Color: #fbbf24
-- @Tag: BOUNCY
-- @Desc: Real spring physics. Windows fall, bounce and settle with proper physics.

hl.curve("spring", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.1 } } })
hl.curve("crouch", { type = "bezier", points = { { 0.1, -0.1 }, { 0.1, 1.0 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 3, bezier = "spring", style = "popin 80%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 3, bezier = "crouch", style = "popin 80%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 3, bezier = "spring", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 2, bezier = "default" })
hl.animation({ leaf = "fadePopupsIn", enabled = true, speed = 2, bezier = "default" })
hl.animation({ leaf = "fadePopupsOut", enabled = true, speed = 2, bezier = "default" })
hl.animation({ leaf = "fadeLayersIn", enabled = true, speed = 2, bezier = "default" })
hl.animation({ leaf = "fadeLayersOut", enabled = true, speed = 2, bezier = "default" })
hl.animation({ leaf = "fadeDim", enabled = true, speed = 3, bezier = "default" })
hl.animation({ leaf = "layers", enabled = true, speed = 3, bezier = "spring", style = "popin" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 4, bezier = "spring", style = "slidevert" })
hl.animation({ leaf = "specialWorkspace", enabled = true, speed = 4, bezier = "spring", style = "slidevert" })
hl.animation({ leaf = "zoomFactor", enabled = true, speed = 3, bezier = "default" })
hl.animation({ leaf = "monitorAdded", enabled = true, speed = 3, bezier = "default" })
