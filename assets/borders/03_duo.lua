-- @Title: Dynamic Duo
-- @Icon: circle-half
-- @Color: #cba6f7
-- @Tag: 2-TONE
-- @Desc: High contrast between Noctalia's Primary and Secondary color.
-- Converted from 03_duo.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { primary, secondary },
                angle = 90
            },
            inactive_border = surface_lowest
        },
        border_size = 1
    }
})

hl.animation({ leaf = "borderangle", enabled = false })
