import type { components } from "@courtpit/api-client";
import { useState } from "react";
import { View } from "react-native";

import { useApi } from "@/api/client";
import { describeError } from "@/api/errors";
import { formatDate } from "@/features/format";
import { nameLookup } from "@/features/players/names";
import { PlayerPicker } from "@/features/players/PlayerPicker";
import { useSignedIn } from "@/session/SessionProvider";
import { space } from "@/theme/tokens";
import { Avatar } from "@/ui/Avatar";
import { Badge } from "@/ui/Badge";
import { Button } from "@/ui/Button";
import { Card } from "@/ui/Card";
import { Section } from "@/ui/Screen";
import { ErrorState } from "@/ui/States";
import { Text } from "@/ui/Text";

import {
  entryState,
  leagueEntries,
  mixedBlocker,
  registrationOpen,
  type EntryState,
  type EntryView,
} from "./entries";
import { InvitationCard } from "./InvitationCard";
import type { LeagueView } from "./league";
import { useEntryActions } from "./useEntryActions";

type PlayerPublic = components["schemas"]["PlayerPublic"];
type Name = (id: string) => string;

/**
 * A league's registration, from the viewer's side: invitations to answer, entering (with a
 * partner, or looking for one), the viewer's entry and its partner, and players looking for one.
 */
export function EntryPanel({ league, now }: { league: LeagueView; now: Date }) {
  const me = useSignedIn().player_id;
  const { $api } = useApi();
  const profile = $api.useQuery("get", "/api/v1/me");
  const entries = $api.useQuery(
    "get",
    "/api/v1/leagues/{id}/entries",
    { params: { path: { id: league.id } } },
    { enabled: league.status === "registration" },
  );

  if (league.status !== "registration" || entries.isPending) return null;
  if (entries.error)
    return <ErrorState error={entries.error} onRetry={() => void entries.refetch()} />;
  const split = leagueEntries(entries.data, me);
  const name = nameLookup(
    entries.data.flatMap((entry) => entry.names),
    me,
  );
  const open = registrationOpen(league, now);
  const own = split.own ? entryState(split.own, me) : null;
  const solo = split.own === null || (own !== null && own.kind !== "entered");
  const blocker = mixedBlocker(league.discipline, profile.data?.player.gender);
  const paired = entries.data.filter((entry) => entry.player_ids.length === 2);
  const unavailable = paired.flatMap((entry) => entry.player_ids);

  if (!open && split.own === null) return null;
  return (
    <>
      <Section title="Registration" aside={<EnteredCount count={split.confirmed} />}>
        {open
          ? split.invitations.map((entry) => (
              <InvitationCard key={entry.id} entry={entry} from={name(entry.created_by)} />
            ))
          : null}
        {split.own && own ? (
          <OwnEntry
            entry={split.own}
            state={own}
            open={open}
            name={name}
            league={league}
            unavailable={unavailable}
          />
        ) : (
          <EnterCard league={league} blocker={blocker} unavailable={unavailable} />
        )}
      </Section>
      {open && league.discipline !== "singles" && solo && !blocker && split.looking.length > 0 ? (
        <Section title="Looking for a partner">
          {split.looking.map((entry) => (
            <LookingRow
              key={entry.id}
              entry={entry}
              name={name}
              ownEntry={split.own?.id ?? null}
              invited={own?.kind === "waiting" && own.partner === entry.created_by}
            />
          ))}
        </Section>
      ) : null}
    </>
  );
}

function EnteredCount({ count }: { count: number }) {
  return (
    <Text variant="caption" tone="textMuted">
      {count === 1 ? "1 entry" : `${count} entries`}
    </Text>
  );
}

/** Entering: singles in one tap; doubles with an invited partner or looking for one. */
function EnterCard({
  league,
  blocker,
  unavailable,
}: {
  league: LeagueView;
  blocker: string | null;
  unavailable: readonly string[];
}) {
  const actions = useEntryActions();
  const [partner, setPartner] = useState<PlayerPublic | null>(null);
  const path = { id: league.id };
  const error = actions.error ? <Text tone="danger">{describeError(actions.error)}</Text> : null;

  if (league.discipline === "singles") {
    return (
      <Card>
        <Text>
          Enter now; the boxes are drawn when the season starts on {formatDate(league.starts_at)}.
        </Text>
        {error}
        <Button
          label="Enter the league"
          loading={actions.register.isPending}
          onPress={() => actions.register.mutate({ params: { path }, body: {} })}
        />
      </Card>
    );
  }
  return (
    <Card>
      {blocker ? (
        <Text tone="textMuted">{blocker}</Text>
      ) : (
        <>
          <PlayerPicker
            label="Your partner"
            value={partner}
            onChange={setPartner}
            exclude={unavailable}
          />
          <Text variant="caption" tone="textMuted">
            Your partner gets an invitation; you’re entered together when they accept.
          </Text>
          {error}
          <Button
            label={partner ? `Enter with ${partner.display_name}` : "Choose a partner to enter"}
            disabled={!partner}
            loading={actions.register.isPending && partner !== null}
            onPress={() =>
              actions.register.mutate({ params: { path }, body: { partner_id: partner?.id } })
            }
          />
          <Button
            label="Enter and look for a partner"
            variant="secondary"
            disabled={actions.register.isPending}
            onPress={() =>
              actions.register.mutate({ params: { path }, body: { looking_for_partner: true } })
            }
          />
        </>
      )}
    </Card>
  );
}

const ownTitle = (state: EntryState, name: Name): string => {
  switch (state.kind) {
    case "entered":
      return state.partner ? `You’re entered with ${name(state.partner)}` : "You’re entered";
    case "waiting":
      return `Waiting for ${name(state.partner)} to accept`;
    case "looking":
      return "You’re looking for a partner";
    case "no_partner":
    case "invited":
      return "Your entry needs a partner";
  }
};

const ownBody = (state: EntryState, league: LeagueView): string => {
  switch (state.kind) {
    case "entered":
      return `The boxes are drawn when the season starts on ${formatDate(league.starts_at)}.`;
    case "waiting":
      return "You can invite someone else instead.";
    case "looking":
      return "Other players looking for a partner can see you. Invite one of them, or anyone else.";
    case "no_partner":
    case "invited":
      return "Your invitation was declined, or your partner entered with someone else.";
  }
};

/** The viewer's entry: where it stands, changing its partner, withdrawing. */
function OwnEntry({
  entry,
  state,
  open,
  name,
  league,
  unavailable,
}: {
  entry: EntryView;
  state: EntryState;
  open: boolean;
  name: Name;
  league: LeagueView;
  unavailable: readonly string[];
}) {
  const actions = useEntryActions();
  const [partner, setPartner] = useState<PlayerPublic | null>(null);
  const [withdrawing, setWithdrawing] = useState(false);
  const path = { id: league.id, entry_id: entry.id };
  const solo = state.kind !== "entered";

  return (
    <Card>
      <Text variant="subheading">{ownTitle(state, name)}</Text>
      <Text variant="label" tone="textMuted">
        {ownBody(state, league)}
      </Text>
      {actions.error ? <Text tone="danger">{describeError(actions.error)}</Text> : null}
      {open && solo ? (
        <>
          <PlayerPicker
            label={state.kind === "waiting" ? "Invite instead" : "Invite a partner"}
            value={partner}
            onChange={setPartner}
            exclude={[...unavailable, ...(state.kind === "waiting" ? [state.partner] : [])]}
          />
          {partner ? (
            <Button
              label={`Invite ${partner.display_name}`}
              loading={actions.partner.isPending}
              onPress={() =>
                actions.partner.mutate(
                  { params: { path }, body: { partner_id: partner.id } },
                  { onSuccess: () => setPartner(null) },
                )
              }
            />
          ) : null}
          {state.kind !== "looking" ? (
            <Button
              label="List me as looking for a partner"
              variant="secondary"
              loading={actions.partner.isPending && partner === null}
              onPress={() =>
                actions.partner.mutate({ params: { path }, body: { looking_for_partner: true } })
              }
            />
          ) : null}
        </>
      ) : null}
      {open ? (
        withdrawing ? (
          <View style={{ gap: space.sm }}>
            <Text variant="label">
              {state.kind === "entered" && state.partner
                ? `Withdraw ${name(state.partner)} and you from the league?`
                : "Withdraw from the league?"}
            </Text>
            <View style={{ flexDirection: "row", gap: space.sm }}>
              <Button
                label="Withdraw"
                variant="danger"
                size="sm"
                loading={actions.withdraw.isPending}
                onPress={() => actions.withdraw.mutate({ params: { path } })}
              />
              <Button
                label="Stay in"
                variant="ghost"
                size="sm"
                onPress={() => setWithdrawing(false)}
              />
            </View>
          </View>
        ) : (
          <Button label="Withdraw" variant="ghost" size="sm" onPress={() => setWithdrawing(true)} />
        )
      ) : null}
    </Card>
  );
}

/** A player looking for a partner, with an invitation one tap away (or already sent). */
function LookingRow({
  entry,
  name,
  ownEntry,
  invited,
}: {
  entry: EntryView;
  name: Name;
  ownEntry: string | null;
  invited: boolean;
}) {
  const actions = useEntryActions();
  const player = entry.created_by;
  const invite = () =>
    ownEntry
      ? actions.partner.mutate({
          params: { path: { id: entry.league_id, entry_id: ownEntry } },
          body: { partner_id: player },
        })
      : actions.register.mutate({
          params: { path: { id: entry.league_id } },
          body: { partner_id: player },
        });
  return (
    <Card>
      <View style={{ flexDirection: "row", alignItems: "center", gap: space.md }}>
        <Avatar id={player} name={name(player)} size={36} />
        <Text weight="semibold" style={{ flex: 1 }}>
          {name(player)}
        </Text>
        {invited ? (
          <View>
            <Badge label="Invited" tone="accent" />
          </View>
        ) : (
          <Button
            label="Invite"
            size="sm"
            variant="secondary"
            loading={actions.partner.isPending || actions.register.isPending}
            onPress={invite}
          />
        )}
      </View>
      {actions.error ? <Text tone="danger">{describeError(actions.error)}</Text> : null}
    </Card>
  );
}
