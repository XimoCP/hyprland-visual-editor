-- @Title: Silk
-- @Icon: feather
-- @Color: #4ade80
-- @Tag: HYBRID
-- @Desc: The refined JaKooLit style. Energetic entries, fleeting exits and stable workspaces.

hl.animation({ leaf = "global", enabled = true, speed = 1, bezier = "default" })

hl.curve("overshot", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.05 } } })
hl.curve("smoothOut", { type = "bezier", points = { { 0.3, 0 }, { 0.3, 1 } } })
hl.curve("crazy", { type = "bezier", points = { { 0.1, 1.1 }, { 0.1, 1.1 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 3, bezier = "overshot", style = "slide" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 3, bezier = "smoothOut", style = "slide" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 3, bezier = "overshot", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 2, bezier = "smoothOut" })
hl.animation({ leaf = "layers", enabled = true, speed = 3, bezier = "overshot", style = "slide" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 4, bezier = "overshot", style = "slide" })
