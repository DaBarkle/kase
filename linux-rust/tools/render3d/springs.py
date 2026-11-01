"""Apple-style springs for keyframes: the same (duration, bounce) form and formula as
`src/ui/tokens.rs` and the island: stiffness (2 pi / d)^2, damping 4 pi (1 - b) / d, mass 1."""

import math


def step(t: float, duration: float, bounce: float) -> float:
    """Progress 0 -> 1 of a spring released at t = 0 (overshoots when bounce > 0)."""
    if t <= 0:
        return 0.0
    k = (2 * math.pi / duration) ** 2
    c = 4 * math.pi * (1 - bounce) / duration
    w0 = math.sqrt(k)
    z = c / (2 * w0)
    if z >= 1:  # critically damped (bounce 0)
        return 1 - (1 + w0 * t) * math.exp(-w0 * t)
    wd = w0 * math.sqrt(1 - z * z)
    return 1 - math.exp(-z * w0 * t) * (math.cos(wd * t) + z * w0 / wd * math.sin(wd * t))


def settle(duration: float, bounce: float, tol: float = 0.002) -> float:
    """When the spring stays within tol of its target for good."""
    t, last_out = 0.0, 0.0
    while t < 10 * duration:
        if abs(1 - step(t, duration, bounce)) > tol:
            last_out = t
        t += 0.001
    return last_out


if __name__ == "__main__":
    for d, b in [(0.5, 0.0), (0.5, 0.3), (0.9, 0.0), (0.55, 0.25)]:
        peak = max(step(i / 1000, d, b) for i in range(3000))
        print(f"{d}/{b}: settles {settle(d, b):.2f} s, peak {peak:.3f}")
