-- @Title: looper
-- @Icon: infinity
-- @Color: #cba6f7
-- @Tag: LOOP
-- @Desc: Looper Aesthetic: Noctalia colors with Joker structure and glow.
-- Converted from 14_looper.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { primary, secondary, tertiary },
                angle = 45
            },
            inactive_border = "#1a002655"
        },
    }
})

hl.config({
    decoration = {
        shadow = {
            enabled = true,
            range = 10,
            render_power = 3,
            color = "rgba(ffffff44)",
            offset = { 0, 0 }
        }
    }
})

hl.curve("nv_looper_flow", { type = "bezier", points = { { 0.4, 0 }, { 0.2, 1 } } })
hl.animation({ leaf = "borderangle", enabled = true, speed = 30, bezier = "nv_looper_flow", style = "loop" })
