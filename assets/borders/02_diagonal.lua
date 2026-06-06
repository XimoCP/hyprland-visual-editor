-- @Title: Diagonal
-- @Icon: slash
-- @Color: #cba6f7
-- @Tag: FADE
-- @Desc: Smooth gradient at a 45° angle using the Noctalia palette.
-- Converted from 02_diagonal.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { primary, surface },
                angle = 45
            },
            inactive_border = surface_lowest
        },
        border_size = 1
    }
})

hl.animation({ leaf = "borderangle", enabled = false })
