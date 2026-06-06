-- @Title: Heartbeat
-- @Icon: activity
-- @Color: #fab387
-- @Tag: EKG
-- @Desc: Electrocardiogram effect. A pulse of color runs through the window when focused.
-- Converted from 06_pulse.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { primary, surface, surface, surface },
                angle = 45
            },
            inactive_border = surface_lowest
        },
        border_size = 1
    }
})

hl.curve("heartbeat", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.0 } } })
hl.animation({ leaf = "borderangle", enabled = true, speed = 40, bezier = "heartbeat", style = "loop" })
hl.animation({ leaf = "border", enabled = true, speed = 1.0, bezier = "default" })
