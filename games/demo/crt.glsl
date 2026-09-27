// Scanlines and a flicker, for the retro canvas. The canvas holds premultiplied colors, so this
// only darkens the color and leaves alpha alone.
extern number time;

vec4 effect(vec4 color, Image tex, vec2 texture_coords, vec2 screen_coords)
{
    vec4 pixel = Texel(tex, texture_coords) * color;
    float dark_line = mod(floor(screen_coords.y / 2.0), 2.0);
    float flicker = 0.95 + 0.05 * sin(time * 20.0);
    pixel.rgb *= (1.0 - 0.35 * dark_line) * flicker;
    return pixel;
}
