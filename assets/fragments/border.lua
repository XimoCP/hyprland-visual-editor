-- @Title: Noctalia Trident
-- @Icon: triangle
-- @Color: #94e2d5
-- @Tag: 3-TONE
-- @Desc: The perfect balance between Primary, Secondary and Tertiary.
-- Converted from 04_tri.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { primary, secondary, tertiary },
                angle = 90
            },
            inactive_border = surface_lowest
        },
        border_size = 1
    }
})

hl.animation({ leaf = "borderangle", enabled = false })
