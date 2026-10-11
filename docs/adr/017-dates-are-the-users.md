# ADR-017: Dates are the user's

Status: accepted, 2026-09-25. Built. Reverses the rule, in AGENTS.md, that
the query parser's and the timeline's calendar boundaries are UTC.

## Context

The query language reads dates (`modified:2026-09-25`), named periods
(`today`, `this-week`, `last-month`) and durations (`modified<7d`); a
clicked timeline bar becomes a query of its dates. All were UTC: in Paris,
"today" began at 02:00 in summer, a file saved at 00:30 was yesterday's,
and a bar labelled in local time named a day it did not count. Chips
offering "Today" make this the first thing a user meets.

Rejected: recording a zone in the query (`tz:Europe/Paris`, or offsets on
dates). Queries are saved in the sidebar and written by the CLI, agents and
chips: a saved "this week" would keep the week of wherever it was saved,
and every writer would have to invent a zone.

## Decision

- **No zone in the query text.** A date or named period is a civil period
  of the user's calendar, turned into instants at each parse (floating
  time, like all-day events), so a saved "today" follows the user across
  zones and clock changes.
- **The calendar** is `skwir_platform::calendar::Calendar`: the system zone
  (`TZ`, else `/etc/localtime`, via jiff and the system tz database) and the
  locale's first weekday (`locale week-1stday first_weekday`, asked once on
  a worker; Monday until then or without `locale`).
- **Days are civil day numbers.** Only `Calendar::day_start` makes an
  instant: local midnight, or the first instant after a gap that skips it.
  A day is 23, 24 or 25 hours.
- **Durations count on the calendar:** `7d` is this time of day seven days
  ago whatever the clocks did, `1mo` the same day last month; what jiff
  cannot read falls back to humantime's exact lengths.
- **Named periods move on:** a query knows when (`Query::next_change`) and
  a location showing it lists again then. The waiting thread checks the
  wall clock at least every ten minutes: the monotonic clock stops during
  sleep.
- **The timeline** buckets by local civil day and starts each bar at its
  first day's first instant, so bar, label and query name the same day.
- **Tests** use an explicit calendar (`lang::parse_in`, `Calendar::utc`,
  `Calendar::named("Europe/Paris", …)`) and cover a clock-change day, a
  midnight that does not exist, a week across the new year, and a week
  starting on Sunday.

## Consequences

- Saved searches with dates now mean the user's zone (a shift of at most
  its UTC offset). No migration: the text is unchanged and the new reading
  is what its author meant.
- Machines in different zones read one query as different instants.
  Explicit offsets (`modified>2026-09-25T14:30+02:00`) would give exact
  instants; not added until needed.
- Until `locale` answers (milliseconds, once per run), weeks may start on
  Monday in a Sunday locale.
- Durations do not roll over: an open `modified<7d` drifts until the
  listing is refreshed.
