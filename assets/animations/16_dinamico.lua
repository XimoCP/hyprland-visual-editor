-- @Title: Dynamic
-- @Icon: adjustments
-- @Color: #f97316
-- @Tag: LIVE
-- @Desc: Organic rhythm. Combination of speed and smooth settling.

---@diagnostic disable: undefined-global
hl.config({ animations = { workspace_wraparound = true, enabled = true } })

hl.curve("pulse", { type = "bezier", points = { { 0.1, 0.9 }, { 0.1, 1.05 } } })
hl.curve("quick", { type = "bezier", points = { { 0.2, 1 }, { 0.2, 1 } } })

hl.animation({ leaf = "windowsIn", enabled = true, speed = 3, bezier = "pulse", style = "slide" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 2, bezier = "quick", style = "slide" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 3, bezier = "pulse", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 2, bezier = "pulse" })
hl.animation({ leaf = "layers", enabled = true, speed = 3, bezier = "pulse", style = "popin" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 4, bezier = "pulse", style = "slidefade 40%" })
