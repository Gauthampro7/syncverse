import math

print("=== ADVERSARIAL STRESS TESTING ===")

# 1. PI Controller with negative initial error e(0) = -450ms
dt = 0.2
Kp = 0.17
Ki = 0.0072
I_max = 32.0
r_min = 0.96
r_max = 1.04
deadband = 0.015

e = -0.450
I = 0.0
steps = int(25.0 / dt)
time_entered_deadband = None
overshoot = False

for step in range(steps):
    t = step * dt
    if abs(e) < deadband:
        r = 1.0
        if time_entered_deadband is None:
            time_entered_deadband = t
    elif abs(e) <= 1.500:
        I_candidate = I + e * dt
        I = max(-I_max, min(I_max, I_candidate))
        delta_r_raw = Kp * e + Ki * I
        r = max(r_min, min(r_max, 1.0 + delta_r_raw))
    else:
        r = 1.0
        e = 0.0
        I = 0.0
    e_next = e - (r - 1.0) * dt
    if e_next > deadband and e <= deadband:
        overshoot = True
    e = e_next

print(f"Negative error e(0) = -450ms: entered deadband at t = {time_entered_deadband}s, overshoot: {overshoot}")
assert time_entered_deadband is not None and time_entered_deadband <= 20.0
assert not overshoot

# 2. Scrubber debouncing at 120 Hz (as specified in Scenario)
events_120hz = []
t = 0.0
while t <= 500.0:
    events_120hz.append(t)
    t += 1000.0 / 120.0

debounce_interval = 100.0
last_seek = -debounce_interval
intermediate_seeks = []
for ev in events_120hz:
    if ev - last_seek >= debounce_interval:
        intermediate_seeks.append(ev)
        last_seek = ev
print(f"120 Hz mouse events: intermediate seeks emitted = {len(intermediate_seeks)}")
assert len(intermediate_seeks) == 5

# 3. High drift close to hard seek threshold: e(0) = 1450ms (Zone 2 boundary)
# At max micro-speed delta_r = 0.04, clearing 1.450s takes minimum 1.450 / 0.04 = 36.25s
e = 1.450
I = 0.0
time_entered = None
overshot = False
steps_40s = int(45.0 / dt)
for step in range(steps_40s):
    t = step * dt
    if abs(e) < deadband:
        r = 1.0
        if time_entered is None:
            time_entered = t
    elif abs(e) <= 1.500:
        I = max(-I_max, min(I_max, I + e * dt))
        r = max(r_min, min(r_max, 1.0 + Kp * e + Ki * I))
    else:
        r = 1.0
        e = 0.0
        I = 0.0
    e_next = e - (r - 1.0) * dt
    if e_next < -deadband and e >= -deadband:
        overshot = True
    e = e_next
print(f"Near-threshold error e(0) = 1450ms: entered deadband at t = {time_entered}s (theoretical min 36.25s), overshoot: {overshot}")
assert time_entered is not None and time_entered <= 40.0
assert not overshot

print("PASS: Adversarial stress tests passed cleanly.")
