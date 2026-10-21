# Eye-based heading skyline

**Geometric mode:** the runtime surface is the analytic WGS84 ellipsoid everywhere. Boundary categories do not change the tangent, range or occlusion. The ellipsoid derivation below remains active; triangle-section candidates, patch masking and numerical boundary fallback describe archived terrain routines/tests only.

The live skyline measurement is the **upper angular outline of the active surface in the vertical plane of the observer's heading**. It reports a straight eye-to-surface range and an elevation angle above local geodetic horizontal. It is recalculated when the observer or active surface changes, independently of render resolution, pitch, telescope zoom and screen pixels.

This quantity is not the farthest visible object in all directions, atmospheric visibility, a surface distance, or the dot's separate forward range. A high ridge can form the skyline while lower visible terrain lies farther away. Optional [weather-informed refraction](refraction.md) replaces the straight heading tangent with actual curved-ray branch searches. Those use 5° exploratory spacing and 22 local bisections, can miss narrower separate branches, and report curved geometric path rather than a universal visibility limit. Scattering, cloud opacity and contrast visibility are not simulated. Outside the four imported regions, the surface remains the reference ellipsoid.

## Heading plane and ellipsoid tangent

Let `O` be the eye ECEF position, `U` geodetic up, and `H` the unit horizontal direction at heading α:

```text
H = north cosα + east sinα
D(θ) = H cosθ + U sinθ
R(s,θ) = O + s D(θ), s ≥ 0
```

The plane normal is `H × U`. Heading selects this plane; pitch does not. The WGS84 portion uses the actual oblate ellipsoid, whose defining axes are sourced in [model.md](model.md), rather than a spherical square-root horizon approximation.

Scale vectors by `S(v) = (v.x/a, v.y/a, v.z/b)`, and write `o=S(O)`, `h=S(H)`, `u=S(U)`, `C=(|o|−1)(|o|+1)`. Tangency requires zero quadratic discriminant:

```text
(o·d)² − C(d·d) = 0,       d = h cosθ + u sinθ

A = (o·u)² − C(u·u)
B = (o·h)(o·u) − C(h·u)
K = (o·h)² − C(h·h)
A t² + 2B t + K = 0,       t = tanθ
```

The implementation uses `q = −B − copysign(sqrt(B²−AK), B)` and roots `q/A`, `K/q`, with linear and zero-`q` cases handled separately. It selects the largest forward tangent angle; forward requires `−(o·d)/(d·d) ≥ 0`. The chosen ray is then passed through the **same canonical Earth intersection** used for dot range. Therefore a hidden ellipsoid tangent is not accepted merely because its analytic angle exists. GPU display LOD does not replace this measurement geometry.

For `C ≤ 0`, the eye is on or inside the reference ellipsoid and there is no exterior tangent calculation. This occurs legitimately above negative ellipsoidal coastal terrain. The algorithm still considers the real mesh and eligible unmasked reference surface.

## Terrain skyline

Each triangle is intersected with the heading plane. Signed plane distances `d₀,d₁` give an edge crossing:

```text
P = P₀ + [d₀/(d₀−d₁)](P₁−P₀)
x = H·(P−O), z = U·(P−O), elevation = atan2(z,x)
```

The resulting segment or coplanar triangle is clipped to `x ≥ 0`. Its elevation extrema occur at the clipped endpoints: along a straight segment, the derivative of `atan2(z,x)` has constant numerator `x z′ − z x′` and positive denominator away from the eye. Thus the algorithm checks actual section endpoints instead of sampling image rows.

A conservative plane/AABB test prunes the existing BVH. The maximum terrain elevation becomes a candidate; the canonical nearest-hit query supplies the range and handles occlusion by other surface geometry. An endpoint ray that is numerically ambiguous may use a downward `10⁻⁹`-radian probe, explicitly classified as an estimate. The original heading-plane ray must have missed, so a shifted hit establishes a local hit/miss interval of that width. This offset is a numerical fallback, not extra terrain geometry.

## Coverage boundaries and estimates

Imported terrain replaces the ellipsoid only within its actual normal-line mesh footprint. This can mask the reference tangent or introduce a visible boundary, especially where coastal ground lies below the ellipsoid. No fictitious wall, ocean surface or continuation of the DEM is inserted at a patch edge.

When analytic candidates do not resolve the remaining reference contribution, the routine searches **2,048 angular intervals** from the unmasked-ellipsoid upper bound, or +90° for an interior eye, down to the best terrain candidate or −90°. Each test performs an actual scene intersection. The highest sampled hit is refined against its preceding miss by at most 40 bisections, stopping at an angular bracket of at most `10⁻¹¹` radians.

The HUD labels this case **Numerical boundary estimate**. A separate unmasked gap narrower than the coarse angular spacing can be missed. The final local hit/miss bracket does **not** certify the global skyline, bound distance error, or characterize source-data uncertainty. A zero search spacing or bracket means that operation was not used or no bracket was established; it does not mean zero uncertainty. Terrain edges and tangent rays can be ill-conditioned; printed decimals are not survey precision. The application exposes both the coarse search spacing and local refinement width instead of describing this fallback as an exact visibility limit.

## Crouching and the screen guide

Crouching moves the eye while preserving the feet. The full construction is repeated from the new origin, changing both perspective and sight obstruction. A lower eye reduces the unobstructed reference-ellipsoid horizon range. With real terrain, the skyline can switch between different ridges or coverage boundaries, so its range need not decrease monotonically.

The L guide uses a separate finite search of the visible, projected canonical Earth outline, including pitch, roll and lens choice. It fills one physical framebuffer row at the highest detected outline point. Its labels report both the chord and WGS84 reference-surface geodesic between the sampled leftmost/rightmost outline points. V measures the gap below that guide in a camera-facing plane at the selected skyline hit's depth. This is not a gravitational height or a certified global outline maximum. The [guide derivation and limits](horizon-guides.md) specify the endpoints, search, HiDPI mapping and unavailable cases.

Implementation: [`terrain/skyline.rs`](../../crates/earth-math/src/terrain/skyline.rs). Independent tests and numerical limits: [verification.md](verification.md).
