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

hl.curve("shimmer", { type = "bezier", points = { { 0.45, 0 }, { 0.55, 1 } } })
hl.animation({ leaf = "borderangle", enabled = true, speed = 60, bezier = "shimmer", style = "loop" })
