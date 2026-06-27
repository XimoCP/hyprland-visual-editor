-- @Title: Cyber Glitch
-- @Icon: bug
-- @Color: #f38ba8
-- @Tag: ERROR
-- @Desc: Aggressive digital glitch effect. Alert colors with ultra-fast rotation.
-- Converted from 09_glitch.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { error, surface, primary, surface },
                angle = 90
            },
            inactive_border = surface_lowest
        },
        border_size = 1
    }
})

hl.curve("rapid", { type = "bezier", points = { { 0, 1 }, { 0, 1 } } })

hl.animation({ leaf = "borderangle", enabled = true, speed = 15, bezier = "rapid", style = "loop" })
