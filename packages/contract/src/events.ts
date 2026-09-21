import { z } from 'zod'
import { ScanStatusSchema } from './scan.ts'

const UintSchema = z.number().int().nonnegative()
const IsoDateTimeSchema = z.iso.datetime()

export const ScanUpdatedPayloadSchema = z.strictObject({
  kind: z.literal('scan.updated'),
  status: ScanStatusSchema,
})

export const ScanCompletedPayloadSchema = z.strictObject({
  kind: z.literal('scan.completed'),
  status: ScanStatusSchema,
})

export const EventGapPayloadSchema = z.strictObject({
  kind: z.literal('eventGap'),
  fromSequence: UintSchema,
  toSequence: UintSchema,
})

export const SessionPayloadSchema = z.strictObject({
  kind: z.literal('session'),
  expiresAt: IsoDateTimeSchema,
})

export const EventPayloadSchema = z.discriminatedUnion('kind', [
  ScanUpdatedPayloadSchema,
  ScanCompletedPayloadSchema,
  EventGapPayloadSchema,
  SessionPayloadSchema,
])

export type EventPayload = z.infer<typeof EventPayloadSchema>

export const EventEnvelopeSchema = z.strictObject({
  sequence: z.number().int().min(1),
  emittedAt: IsoDateTimeSchema,
  payload: EventPayloadSchema,
})

export type EventEnvelope = z.infer<typeof EventEnvelopeSchema>
