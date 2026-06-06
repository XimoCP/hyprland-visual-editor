-- @Title: Infinity
-- @Icon: infinity
-- @Color: #cba6f7
-- @Tag: LOOP
-- @Desc: Fluid loop of Noctalia colors. Constant and elegant rotation.
-- Converted from 07_infinity.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { primary, secondary, tertiary, error, primary, secondary, tertiary, error },
                angle = 45
            },
            inactive_border = surface_lowest
        },
        border_size = 1
    }
})
hl.curve("linear", { type = "bezier", points = { { 0.0, 0.0 }, { 1.0, 1.0 } } })
hl.animation({ leaf = "borderangle", enabled = true, speed = 50, bezier = "linear", style = "loop" })
