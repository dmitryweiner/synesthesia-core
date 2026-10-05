// One Gray-Scott substep. State texture: R = u, G = v.
//
// The stencil reads with texelFetch, not texture(): it lands exactly on
// texel centers, so a bilinear filter has nothing to interpolate and its
// cost is pure waste — and on a software rasterizer this pass, run
// `reaction.speed` times per frame, is the largest single item in the frame.
// The integer coordinates are clamped by hand, which is the same zero-flux
// boundary CLAMP_TO_EDGE gave the sampled version.
//
// feed/kill are perturbed per-pixel by the Field variation paramfield, which
// is a smooth half-resolution texture and so is still sampled with a filter
// (a 1x1 zero texture when that card is off — an exact no-op).
in vec2 vUv;
out vec4 fragColor;

uniform sampler2D uState;
uniform sampler2D uParamField;
uniform float uFeed;
uniform float uKill;
uniform float uDiffU;
uniform float uDiffV;
uniform float uDt;

vec2 tap(ivec2 c, ivec2 hi) {
  return texelFetch(uState, clamp(c, ivec2(0), hi), 0).rg;
}

void main() {
  ivec2 c = ivec2(gl_FragCoord.xy);
  ivec2 hi = textureSize(uState, 0) - ivec2(1);
  vec2 here = tap(c, hi);

  // 9-point weighted Laplacian (more isotropic than the plain 5-point stencil).
  vec2 lap = -here;
  lap += (tap(c + ivec2(-1, -1), hi) + tap(c + ivec2(1, -1), hi)
        + tap(c + ivec2(-1, 1), hi) + tap(c + ivec2(1, 1), hi)) * 0.05;
  lap += (tap(c + ivec2(0, -1), hi) + tap(c + ivec2(-1, 0), hi)
        + tap(c + ivec2(1, 0), hi) + tap(c + ivec2(0, 1), hi)) * 0.2;

  float u = here.r;
  float v = here.g;
  vec2 fieldOffset = texture(uParamField, vUv).rg;
  float feed = clamp(uFeed + fieldOffset.r, 0.0, 1.0);
  float kill = clamp(uKill + fieldOffset.g, 0.0, 1.0);
  float reaction = u * v * v;
  float du = uDiffU * lap.r - reaction + feed * (1.0 - u);
  float dv = uDiffV * lap.g + reaction - (feed + kill) * v;
  float nu = clamp(u + du * uDt, 0.0, 1.0);
  float nv = clamp(v + dv * uDt, 0.0, 1.0);
  fragColor = vec4(nu, nv, 0.0, 1.0);
}
