import type { LeagueView } from "@/features/leagues/league";

import { endOf, startOf } from "@/ui/calendar";
import {
  STARTER_FORMAT,
  checkLeague,
  createBody,
  formFromLeague,
  newLeagueForm,
  patchBody,
} from "./leagueForm";

describe("newLeagueForm", () => {
  it("opens registration today for two weeks and runs a twelve-week season", () => {
    const form = newLeagueForm("2026-10-05");
    expect(form).toMatchObject({
      opens: "2026-10-05",
      closes: "2026-10-18",
      starts: "2026-10-20",
      ends: "2027-01-11",
      boxMin: 6,
      boxMax: 8,
      format: { kind: "default" },
    });
    expect(checkLeague({ ...form, name: "Winter Singles" })).toEqual({});
  });
});

describe("checkLeague", () => {
  const form = { ...newLeagueForm("2026-10-05"), name: "Winter Singles" };

  it("orders the dates like the server", () => {
    expect(checkLeague({ ...form, closes: "2026-10-04" }).dates).toMatch(
      /on or after the day it opens/,
    );
    expect(checkLeague({ ...form, starts: form.closes }).dates).toMatch(
      /after registration closes/,
    );
    expect(checkLeague({ ...form, ends: "2026-10-19" }).dates).toMatch(/ends on or after/);
    expect(checkLeague({ ...form, opens: form.closes }).dates).toBeUndefined();
  });

  it("checks the name and box sizes", () => {
    expect(checkLeague({ ...form, name: " " }).name).toBeDefined();
    expect(checkLeague({ ...form, boxMin: 9 }).boxes).toBeDefined();
    expect(checkLeague({ ...form, boxMin: 1 }).boxes).toBeDefined();
    expect(checkLeague({ ...form, boxMax: 17 }).boxes).toBeDefined();
  });
});

describe("createBody", () => {
  it("turns days into instants and the format choice into an override", () => {
    const form = { ...newLeagueForm("2026-10-05"), name: " Winter Singles " };
    const body = createBody(form);
    expect(body).toMatchObject({
      name: "Winter Singles",
      registration_opens_at: startOf("2026-10-05").toISOString(),
      registration_closes_at: endOf("2026-10-18").toISOString(),
      starts_at: startOf("2026-10-20").toISOString(),
      ends_at: endOf("2027-01-11").toISOString(),
      match_format: null,
    });
    expect(
      createBody({ ...form, format: { kind: "custom", format: STARTER_FORMAT } }).match_format,
    ).toEqual(STARTER_FORMAT);
  });
});

describe("patchBody", () => {
  const form = { ...newLeagueForm("2026-10-05"), name: "Winter Singles" };
  const created = createBody(form);
  const league: LeagueView = {
    id: "l1",
    ...created,
    discipline: "singles",
    box_min_size: 6,
    box_max_size: 8,
    status: "draft",
    match_format: STARTER_FORMAT,
    created_at: "2026-10-05T00:00:00Z",
  } as LeagueView;

  it("reads a league back into the same days and sends nothing when untouched", () => {
    const back = formFromLeague(league);
    expect(back).toMatchObject({
      opens: form.opens,
      closes: form.closes,
      starts: form.starts,
      ends: form.ends,
    });
    expect(patchBody(league, back)).toEqual({});
  });

  it("sends only the changes, and the club default as null", () => {
    const back = formFromLeague(league);
    expect(
      patchBody(league, {
        ...back,
        closes: "2026-10-25",
        starts: "2026-10-27",
        format: { kind: "default" },
      }),
    ).toEqual({
      registration_closes_at: endOf("2026-10-25").toISOString(),
      starts_at: startOf("2026-10-27").toISOString(),
      match_format: null,
    });
  });
});
