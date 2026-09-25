# ADR-017: Dates are the user's

Status: accepted, 2026-09-25. Reverses the rule, in AGENTS.md, that the
query parser's and the timeline's calendar boundaries are UTC.

## Context

The query language reads dates (`modified:2026-09-25`), named periods
(`modified:today`, `this-week`, `last-month`) and durations
(`modified<7d`). The timeline draws bars of days, weeks, months and years,
and a bar clicked becomes a query of its dates. All of them were UTC: a
day began at 00:00 UTC, a week on the Monday of UTC.

UTC is not the day a person lives. In Paris, "today" began at 02:00 in
summer, so a file saved at 00:30 was yesterday's; a bar's label, drawn in
the system time zone, named a day the bar did not count. A chip designer
that offers "Today" and "This week" to people who write no syntax makes
the mismatch the first thing they meet.

The question is where the zone is recorded. A query is text that a pane
shows, that is saved in the sidebar, that the CLI, an agent or a chip
writes. Writing a zone into it (`tz:Europe/Paris`, or offsets on every
date) would make a saved "this week" keep the week of wherever it was
saved, and would give the designer a zone to invent for every date it
writes.

## Decision

- The query text records no time zone. A date or a named period is a
  civil period of the user's calendar, turned into instants each time the
  query is parsed: floating time, as a calendar's all-day events are. A
  saved "today" follows the user across zones and clock changes.
- The calendar is `bibli_platform::calendar::Calendar`: the system's time
  zone (`TZ`, else `/etc/localtime`, through jiff and the system's tz
  database) and the locale's first weekday (`LC_TIME`, from
  `locale week-1stday first_weekday`, asked once on a worker; Monday until
  it answers, or where there is no `locale`). Days are civil day numbers;
  only `Calendar::day_start` makes one an instant, the first of the day:
  local midnight, or the first instant after a gap that skips it. A day is
  23, 24 or 25 hours.
- Durations are read on the calendar: `7d` is this time of day seven days
  ago whatever the clocks did, `1mo` the same day last month. What jiff
  does not read falls back to humantime's exact lengths.
- A parsed query knows when its named periods move on
  (`Query::next_change`); a location showing one lists again then, on a
  thread that looks at the wall clock at least every ten minutes, since
  the monotonic clock a wait counts stops while the machine sleeps.
- The timeline folds entries by the civil day they fall on in the zone and
  starts each bar at its first day's first instant, so a bar, its label and
  the query it becomes name the same day.
- Tests parse with an explicit calendar (`lang::parse_in`, `Calendar::utc`,
  `Calendar::named("Europe/Paris", …)`) and test boundary instants: a day
  of the clocks changing, a midnight that does not exist, a week across the
  new year, a locale whose week starts on Sunday.

## Consequences

- A saved search with dates now means them in the user's zone: it moves by
  at most the zone's offset from UTC. No migration: the text is unchanged
  and the new reading is the one its author meant.
- Two machines in different zones read the same query text as different
  instants, as they read "today". A caller that needs an exact instant
  needs a syntax for one; timestamps with an explicit offset
  (`modified>2026-09-25T14:30+02:00`) are the way, not added until needed.
- The week's first day comes from a process, so the first queries after
  start may count weeks from Monday on a Sunday locale, for as long as
  `locale` takes to answer (milliseconds), once per run.
- Durations do not roll over: `modified<7d` left open drifts until the
  listing is refreshed, as before.
