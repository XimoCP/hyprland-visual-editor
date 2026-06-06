-- @Title: Waterfall
-- @Icon: arrow-down
-- @Color: #cba6f7
-- @Tag: FLOW
-- @Desc: Dynamic border with vertical gradient using the Noctalia palette.
-- Converted from 01_cascade.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { primary, surface },
                angle = 90
            },
            inactive_border = surface_lowest
        },
        border_size = 1
    }
})

hl.animation({ leaf = "borderangle", enabled = false })
