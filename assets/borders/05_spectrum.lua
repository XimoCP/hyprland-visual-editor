-- @Title: Spectrum
-- @Icon: aperture
-- @Color: #fab387
-- @Tag: RAINBOW
-- @Desc: The complete Noctalia color cycle (Static).
-- Converted from 05_spectrum.conf
-- =====================================
---@diagnostic disable: undefined-global

hl.config({
    general = {
        col = {
            active_border = {
                colors = { primary, secondary, tertiary, error },
                angle = 45
            },
            inactive_border = surface_lowest
        },
        border_size = 1
    }
})

hl.animation({ leaf = "borderangle", enabled = false })
