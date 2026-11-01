# Kase journeys

What the island should do in each real-life moment with AirPods, so it tells **one calm
story per thing you do** and never jumps between messages. The look of each presentation
is in [DESIGN.md](DESIGN.md).

Each journey has a script in `tools/visual-qa/journeys/` (timings from a real AirPods 4
capture) and is recorded frame by frame with `just visual-journeys`. The island's
presentation logic is tested against these storyboards (`journeys_tests.rs`).

## What Kase hears (measured 2026-11-01, AirPods 4 ANC)

- A pod's battery update follows its ear or case change by 46 to 113 ms.
- The same values are repeated for up to 1.5 s within one burst.
- Separate actions by a person are at least 1.28 s apart.
- Out of the ear to "in the case" takes 0.95 to 2 s.
- The case reads 0% "disconnected" while both pods are out, and "unknown" for about 2 s
  after the first pod goes in.
- Closing the lid sends a fixed burst of 6 packets within 120 ms; the link drops about
  12 s later.

## The rules that make it calm

1. **Settle.** Changes are gathered for 300 ms after the last meaningful change (at most
   1.5 s after the first), then judged together. Repeats with unchanged values are ignored.
2. **One action, one presentation.** An alert of the same kind merges (left and right
   charging becomes "AirPods charging").
3. **Update in place.** While a card is showing, changes update it (a ring turns green, a
   number rolls) and keep it up for at least 3 s after the last change.
4. **Minimum time on screen.** A new presentation waits until the current one has been up
   1.2 s. At most one waits; the newest wins.
5. **Quiet endings.** Closing the lid folds the island; no "Disconnected" alert for the
   next 30 s.
6. **Unknown is not news.** An unknown or 0% "disconnected" case reading keeps the last
   value, dimmed, and never pops anything.
7. **Low battery once.** One orange alert at 20% or less and one red at 10% or less per
   discharge. It re-arms only after charging or rising 5 points above the line.

## Storyboards

| # | You do | The island shows | For | Must never |
| --- | --- | --- | --- | --- |
| 1 | Open the case at the desk (first connect of the day) | The case card with the "spin and open" clip; rings fill | 6 s, cut to 2 s once a pod is out | Also show a "Connected" alert |
| 2 | Take both pods out and put them in your ears | The card stays (no new pop); rings update in place; then folds to compact or hides, per mode | The card's remaining time | Pop once per pod |
| 3 | Put the pods back one at a time and close the lid | First pod in: if the card is open its ring turns green in place, otherwise one alert "Left AirPod charging". Second pod soon after: updates in place. Both in: the case card with the "put away" clip. Lid closed: the island folds away quietly | Alert 2.2 s; card 3 s after the last change | Left alert then right alert back to back; a "Disconnected" alert 12 s after the lid closed |
| 4 | One pod in the case, one in your ear | One alert "Right AirPod charging"; compact shows R charging | 2.2 s | Pop again when the case level arrives |
| 5 | Put the case on a charger | One alert "Case charging" | 3 s | Repeat on every battery packet |
| 6 | Battery gets low while you're gaming | One orange alert at 20% or less, one red at 10% or less, per discharge | 5 s | Repeat when the level wobbles between 20 and 21 |
| 7 | The iPhone takes the audio, then gives it back | "Playing on iPhone", then "Back on this PC" | 3.5 s / 2.6 s | "Disconnected" replacing it within 1.2 s |
| 8 | Walk out of range, then come back | "Disconnected" (only if the lid wasn't just closed); on return, compact or the card, per mode | 2.6 s | |
| 9 | Switch the mic with a stem hold | "AirPods Mic" or "Desk Mic"; a shake if the AirPods mic isn't available | 2.2 s / 3 s | |
| 10 | Change noise control quickly | One alert showing the final mode | 2.2 s from the last change | Three alerts in a row |

## Today (Kase 0.11, recorded 2026-11-01)

What the journey suite recorded before the calm island work, to compare against:

- **3, put away:** "Left AirPod charging" at 0.96 s, replaced by the card at 2.96 s when
  the case level arrives, then the card's text changes to "Case open" at 5.16 s, folds at
  6.97 s. Three presentations for one action, and the charging alert moves for 4.2 s.
