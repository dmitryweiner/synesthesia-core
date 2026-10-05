// Velocity field for advection: curl noise (divergence-free) plus a constant
// drift ("gravity"). Recomputed once per rendered frame. Noise is sampled in
// aspect-corrected space (isotropic on screen) and the velocity's x is
// mapped back to UV units, since advect.frag steps in UV.
in vec2 vUv;
out vec4 fragColor;

uniform float uCurlStrength;
uniform float uCurlScale;
uniform vec2 uDrift;
uniform float uEvolveT;
uniform float uAspect; // grid width / height

void main() {
  vec2 p = vUv * vec2(uAspect, 1.0) * uCurlScale + uEvolveT;
  vec2 vel = curlNoise(p, 0.05) * uCurlStrength + uDrift;
  vel.x /= uAspect;
  fragColor = vec4(vel, 0.0, 1.0);
}
