-- @Title: Faded
-- @Icon: cloud
-- @Color: #cbd5e1
-- @Tag: GHOST
-- @Desc: Spectral materialization. The windows appear smoothly without barely moving.
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    animations = {
        workspace_wraparound = false,
        enabled = true
    }
})

hl.curve("phantom", { type = "bezier", points = { { 0.4, 0 }, { 0.6, 1 } } })
hl.curve("mist", { type = "bezier", points = { { 0.2, 0.8 }, { 0.2, 1 } } })
hl.curve("ethereal", { type = "bezier", points = { { 0.5, 0.05 }, { 0.1, 1 } } })


hl.animation({ leaf = "windowsIn", enabled = true, speed = 7, bezier = "phantom", style = "popin 90%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 6, bezier = "phantom", style = "popin 95%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 7, bezier = "ethereal", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 8, bezier = "phantom" })
hl.animation({ leaf = "layers", enabled = true, speed = 6, bezier = "phantom", style = "popin" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 8, bezier = "phantom", style = "fade" })
