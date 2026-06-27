-- @Title: Neon
-- @Icon: blur
-- @Color: #cba6f7
-- @Tag: GLITCH
-- @Desc: "Cyberpunk" effect. The edge light flickers, moving back and forth like unstable electricity.
-- Converted from 08_neon.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { primary, surface, primary, surface, primary, surface },
                angle = 45
            },
            inactive_border = surface_lowest
        },
        border_size = 1
    }
})

hl.curve("glitch", { type = "bezier", points = { { 0.1, 1.5 }, { 0.9, -0.5 } } })
hl.animation({ leaf = "borderangle", enabled = true, speed = 40, bezier = "glitch", style = "loop" })
