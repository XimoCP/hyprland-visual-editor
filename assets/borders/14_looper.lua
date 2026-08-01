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
            range = 20,
            render_power = 4,
            color = "rgba(ffffff44)",
            color_inactive = "rgba(ffffff00)", -- fundido a invisible al perder foco
            offset = { 0, 0 }
        }
    }
})

hl.config({
    windowrulev2 = {
        "noshadow, focus:0",
        "dim_around, floating:1"
    }
})

hl.curve("nv_looper_flow", { type = "bezier", points = { { 0.4, 0 }, { 0.2, 1 } } })
hl.animation({ leaf = "borderangle", enabled = true, speed = 30, bezier = "nv_looper_flow", style = "loop" })
hl.animation({ leaf = "border", enabled = true, speed = 2, bezier = "default" })
hl.animation({ leaf = "fadeShadow", enabled = true, speed = 10, bezier = "default" })
