"""Quantify angular-plane eclipse area against two spherical reference integrals.

Uniform radiance and smooth spheres only. These comparisons deliberately omit
limb darkening, lunar relief, refraction and scattering. They are a finite
parameter sweep, not a certified supremum over every eclipse geometry.
"""
from __future__ import annotations

import json
import math
from functools import lru_cache
import numpy as np

from verify_eclipses import chord_overlap


@lru_cache
def quadrature(nodes):
    return np.polynomial.legendre.leggauss(nodes)


def spherical_fractions(alpha: float, beta: float, separation: float, nodes=256):
    """Return occulted solid-angle and Sun-normal projected-flux fractions."""
    if separation >= alpha + beta:
        return 0.0, 0.0
    if separation + alpha <= beta:
        return 1.0, 1.0
    if separation == 0:
        return ((math.sin(beta/2)/math.sin(alpha/2))**2,
                (math.sin(beta)/math.sin(alpha))**2)
    cuts = sorted(set([0.0, alpha, min(alpha, abs(beta-separation)),
                       min(alpha, beta+separation)]))
    gauss, weights = quadrature(nodes)
    cap, flux = 0.0, 0.0
    for a,b in zip(cuts,cuts[1:]):
        if a == b:
            continue
        # Smooth square-root angular contact endpoints before quadrature.
        t = gauss*math.pi/2
        theta = (a+b)/2+(b-a)/2*np.sin(t)
        jacobian = (b-a)/2*np.cos(t)*math.pi/2
        st = np.sin(theta/2)**2
        sd = math.sin(separation/2)**2
        sb = math.sin(beta/2)**2
        # (cos(beta)-cos(theta)*cos(delta))/(sin(theta)*sin(delta)),
        # evaluated without subtracting nearly equal cosines.
        threshold = (2*st+2*sd-4*st*sd-2*sb)/(np.sin(theta)*math.sin(separation))
        phi = 2*np.arccos(np.clip(threshold,-1,1))
        value = phi*np.sin(theta)*jacobian*weights
        cap += float(np.sum(value))
        flux += float(np.sum(value*np.cos(theta)))
    return cap/(4*math.pi*math.sin(alpha/2)**2), flux/(math.pi*math.sin(alpha)**2)


def main():
    worst_cap = (0.0,None)
    worst_flux = (0.0,None)
    max_refinement = 0.0
    count = 0
    # Covers and slightly exceeds terrestrial solar angular radii and ordinary
    # terrestrial lunar/solar apparent-radius ratios.
    for alpha in [0.0045,0.00465,0.0048]:
        for ratio in [0.9,0.95,0.99,1.0,1.01,1.05,1.1]:
            inner,outer = abs(1-ratio),1+ratio
            for d in sorted(set([0.0,inner]+[
                inner+(outer-inner)*i/20 for i in range(1,20)
            ]+[outer])):
                plane = chord_overlap(ratio,d)
                cap,flux = spherical_fractions(alpha,alpha*ratio,alpha*d)
                check = spherical_fractions(alpha,alpha*ratio,alpha*d,nodes=512)
                max_refinement = max(max_refinement,abs(cap-check[0]),abs(flux-check[1]))
                case = {"sun_radius_rad":alpha,"moon_sun_radius_ratio":ratio,
                        "separation_in_sun_radii":d,"angular_plane":plane,
                        "spherical_cap":cap,"projected_uniform_radiance":flux}
                if abs(plane-cap)>worst_cap[0]:
                    worst_cap=(abs(plane-cap),case)
                if abs(plane-flux)>worst_flux[0]:
                    worst_flux=(abs(plane-flux),case)
                count+=1
    print(json.dumps({
        "cases":count,
        "maximum_sampled_absolute_fraction_difference_spherical_cap":worst_cap,
        "maximum_sampled_absolute_fraction_difference_projected_flux":worst_flux,
        "maximum_256_to_512_node_change":max_refinement,
        "scope":"finite smooth-sphere parameter sweep; not a global certified error bound",
    },indent=2))


if __name__ == "__main__":
    main()
