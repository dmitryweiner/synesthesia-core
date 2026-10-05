// Initial state for the reaction-diffusion field. u defaults to the
// substrate value (1.0); v starts at 0 except where a seed spot places it —
// without some v > 0 somewhere, u=1,v=0 is a stable fixed point. Spot
// centers are Math.random() on the CPU and re-rolled on every reseed.
in vec2 vUv;
out vec4 fragColor;

uniform int uSpotCount;
uniform vec2 uSpots[24];
uniform float uSpotRadius;
uniform float uAspect; // grid width / height — spots stay round on non-square grids

void main() {
  vec2 uv = vUv;
  float u = 1.0;
  float v = 0.0;
  for (int i = 0; i < 24; i++) {
    if (i >= uSpotCount) break;
    vec2 dv = (uv - uSpots[i]) * vec2(uAspect, 1.0);
    float d = length(dv);
    float blob = smoothstep(uSpotRadius, uSpotRadius * 0.2, d);
    v = max(v, blob * 0.5);
    u = mix(u, 0.5, blob);
  }
  fragColor = vec4(u, v, 0.0, 1.0);
}
