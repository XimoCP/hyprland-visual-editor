-- @Title: Golden Luxury
-- @Icon: crown
-- @Color: #FFD700
-- @Tag: PRO
-- @Desc: 24k gold. An intense white reflection travels over a real gold surface.
-- Converted from 10_golden.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { "0xffC5A000", "0xffFFD700", "0xffFFFFfF", "0xffFFD700", "0xffC5A000" },
                angle = 45
            },
            inactive_border = surface_lowest
        },
        border_size = 1
    }
})

hl.config({
    windowrulev2 = {
        "noshadow, focus:0",
        "dim_around, floating:1"
    }
})

hl.curve("shimmer", { type = "bezier", points = { { 0.45, 0 }, { 0.55, 1 } } })
hl.animation({ leaf = "borderangle", enabled = true, speed = 30, bezier = "shimmer", style = "loop" })
hl.animation({ leaf = "border", enabled = true, speed = 2, bezier = "default" })
hl.animation({ leaf = "fadeShadow", enabled = true, speed = 2, bezier = "default" })
