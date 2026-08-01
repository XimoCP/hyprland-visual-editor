-- @Title: Neon Cyber-Glow (Dual)
-- @Icon: bolt
-- @Color: #00fff7
-- @Tag: GLOW
-- @Desc: Two-color glow simulation using a white/violet light base.
-- Converted from 12_neon_cyberpunk.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { "rgba(00fff7ff)", "rgba(1a0026ff)", "rgba(ff00ffff)" },
                angle = 45
            },
            inactive_border = "rgba(0f0f2655)"
        },
        border_size = 2
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

hl.curve("nv_neon_flow", { type = "bezier", points = { { 0.4, 0 }, { 0.2, 1 } } })
hl.animation({ leaf = "borderangle", enabled = true, speed = 30, bezier = "nv_neon_flow", style = "loop" })
hl.animation({ leaf = "border", enabled = true, speed = 30, bezier = "default" })
hl.animation({ leaf = "fadeShadow", enabled = true, speed = 15, bezier = "default" })
