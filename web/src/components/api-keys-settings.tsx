import React, { useEffect, useMemo, useState } from 'react'
import { Badge, Button, Callout, Card, Flex, Switch, Text, TextField } from '@radix-ui/themes'
import { api, ApiError } from '../lib/api'

type ApiKeyRecord = {
  id: number
  label: string
  prefix: string
  created_at?: string | null
  revoked_at?: string | null
  expires_at?: string | null
  last_used_at?: string | null
  rotated_from_key_id?: number | null
  scopes?: string[]
}

type ApiKeyListResponse = {
  ok?: boolean
  keys?: ApiKeyRecord[]
}

type ApiKeyMutationResponse = {
  ok?: boolean
  api_key?: string
  prefix?: string
  scopes?: string[]
  expires_at?: string | null
}

type IssuedKey = {
  apiKey: string
  prefix: string
  scopes: string[]
  expiresAt?: string | null
}

type ScopeToggles = {
  read: boolean
  write: boolean
  admin: boolean
  approvals: boolean
}

const DEFAULT_SCOPE_TOGGLES: ScopeToggles = {
  read: true,
  write: true,
  admin: false,
  approvals: false,
}

export function ApiKeysSettings({
  open,
  authenticated,
}: {
  open: boolean
  authenticated: boolean
}) {
  const [keys, setKeys] = useState<ApiKeyRecord[]>([])
  const [loading, setLoading] = useState<boolean>(false)
  const [busyAction, setBusyAction] = useState<string>('')
  const [error, setError] = useState<string>('')
  const [notice, setNotice] = useState<string>('')
  const [createLabel, setCreateLabel] = useState<string>('mission-control')
  const [expiresDays, setExpiresDays] = useState<string>('')
  const [scopeToggles, setScopeToggles] = useState<ScopeToggles>(DEFAULT_SCOPE_TOGGLES)
  const [issuedKey, setIssuedKey] = useState<IssuedKey | null>(null)

  useEffect(() => {
    if (!open || !authenticated) return
    void loadKeys()
  }, [open, authenticated])

  const sortedKeys = useMemo(() => {
    return [...keys].sort((left, right) => {
      const leftRevoked = Boolean(left.revoked_at)
      const rightRevoked = Boolean(right.revoked_at)
      if (leftRevoked !== rightRevoked) return leftRevoked ? 1 : -1

      const leftCreated = Date.parse(left.created_at || '')
      const rightCreated = Date.parse(right.created_at || '')
      if (Number.isFinite(leftCreated) && Number.isFinite(rightCreated) && leftCreated !== rightCreated) {
        return rightCreated - leftCreated
      }

      return left.label.localeCompare(right.label)
    })
  }, [keys])

  function selectedScopes(): string[] {
    const scopes: string[] = []
    if (scopeToggles.read) scopes.push('operator.read')
    if (scopeToggles.write) scopes.push('operator.write')
    if (scopeToggles.admin) scopes.push('operator.admin')
    if (scopeToggles.approvals) scopes.push('operator.approvals')
    return scopes
  }

  async function loadKeys(): Promise<void> {
    setLoading(true)
    setError('')
    try {
      const data = await api<ApiKeyListResponse>('/api/auth/api_keys')
      setKeys(Array.isArray(data.keys) ? data.keys : [])
    } catch (e) {
      if (e instanceof ApiError && e.status === 403) {
        setError('Current session cannot manage API keys. Sign in with an operator session.')
        return
      }
      if (e instanceof ApiError && e.status === 401) {
        setError('Sign in again to manage API keys.')
        return
      }
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  async function createKey(): Promise<void> {
    const label = createLabel.trim()
    const scopes = selectedScopes()
    const trimmedExpiry = expiresDays.trim()
    const expiryValue = trimmedExpiry ? Number(trimmedExpiry) : null

    if (!label) {
      setError('Label is required.')
      return
    }
    if (scopes.length === 0) {
      setError('Select at least one scope.')
      return
    }
    if (trimmedExpiry && (!Number.isFinite(expiryValue) || expiryValue === null || expiryValue < 1)) {
      setError('Expiry days must be a positive number.')
      return
    }

    setBusyAction('create')
    setError('')
    setNotice('')
    try {
      const data = await api<ApiKeyMutationResponse>('/api/auth/api_keys', {
        method: 'POST',
        body: JSON.stringify({
          label,
          scopes,
          expires_days: expiryValue,
        }),
      })
      if (!data.api_key) {
        throw new Error('missing api_key in create response')
      }
      setIssuedKey({
        apiKey: data.api_key,
        prefix: String(data.prefix || ''),
        scopes: Array.isArray(data.scopes) ? data.scopes : scopes,
        expiresAt: data.expires_at ?? null,
      })
      setNotice(`Created API key "${label}". Save the secret now; it is only shown once.`)
      await loadKeys()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusyAction('')
    }
  }

  async function revokeKey(item: ApiKeyRecord): Promise<void> {
    if (!window.confirm(`Revoke API key "${item.label}" (${item.prefix})?`)) return

    setBusyAction(`revoke:${item.id}`)
    setError('')
    setNotice('')
    try {
      await api(`/api/auth/api_keys/${item.id}`, {
        method: 'DELETE',
      })
      setNotice(`Revoked API key "${item.label}".`)
      await loadKeys()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusyAction('')
    }
  }

  async function rotateKey(item: ApiKeyRecord): Promise<void> {
    if (!window.confirm(`Rotate API key "${item.label}" (${item.prefix})? The current key will be revoked.`)) return

    setBusyAction(`rotate:${item.id}`)
    setError('')
    setNotice('')
    try {
      const data = await api<ApiKeyMutationResponse>(`/api/auth/api_keys/${item.id}/rotate`, {
        method: 'POST',
        body: JSON.stringify({
          label: item.label,
          scopes: Array.isArray(item.scopes) ? item.scopes : [],
        }),
      })
      if (!data.api_key) {
        throw new Error('missing api_key in rotate response')
      }
      setIssuedKey({
        apiKey: data.api_key,
        prefix: String(data.prefix || ''),
        scopes: Array.isArray(data.scopes) ? data.scopes : (item.scopes || []),
        expiresAt: data.expires_at ?? null,
      })
      setNotice(`Rotated API key "${item.label}". Save the new secret now; the old one is revoked.`)
      await loadKeys()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusyAction('')
    }
  }

  async function copyIssuedKey(): Promise<void> {
    if (!issuedKey?.apiKey) return
    try {
      await navigator.clipboard.writeText(issuedKey.apiKey)
      setNotice('Copied API key to clipboard.')
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Clipboard copy failed.')
    }
  }

  function updateScopeToggle(key: keyof ScopeToggles, checked: boolean): void {
    setScopeToggles((prev) => ({ ...prev, [key]: checked }))
  }

  return (
    <div className="space-y-4">
      <Callout.Root size="1" variant="soft">
        <Callout.Text>
          For Mission Control, create a key with <code>operator.read</code> and <code>operator.write</code>.
          Browser sessions already have admin rights after sign-in; these keys are for WebSocket clients, scripts, and automation.
        </Callout.Text>
      </Callout.Root>

      {issuedKey ? (
        <Card className="p-3">
          <Flex align="center" justify="between" gap="3" wrap="wrap">
            <div>
              <Text size="2" weight="bold">Newest secret</Text>
              <Text size="1" color="gray" className="mt-1 block">
                This raw key is only returned once. Store it now.
              </Text>
            </div>
            <Button variant="soft" onClick={() => void copyIssuedKey()}>
              Copy
            </Button>
          </Flex>
          <TextField.Root
            className="mt-3"
            value={issuedKey.apiKey}
            readOnly
          />
          <Flex gap="2" wrap="wrap" className="mt-3">
            <Badge variant="soft" color="green">{issuedKey.prefix}</Badge>
            {issuedKey.scopes.map((scope) => (
              <Badge key={scope} variant="soft" color="blue">{scope}</Badge>
            ))}
            <Badge variant="soft" color={issuedKey.expiresAt ? 'amber' : 'gray'}>
              expires {formatTimestamp(issuedKey.expiresAt, 'never')}
            </Badge>
          </Flex>
        </Card>
      ) : null}

      <Card className="p-3">
        <Text size="3" weight="bold">Create API key</Text>
        <Text size="1" color="gray" className="mt-1 block">
          Changes here apply immediately. You do not need to use the Settings Save button for API keys.
        </Text>
        <div className="mt-4 grid grid-cols-1 gap-3 md:grid-cols-2">
          <Card className="p-3">
            <Text size="2" weight="medium">Label</Text>
            <Text size="1" color="gray" className="mt-1 block">
              Human-friendly name for the consumer of this key.
            </Text>
            <TextField.Root
              className="mt-3"
              value={createLabel}
              onChange={(e) => setCreateLabel(e.target.value)}
              placeholder="mission-control"
            />
          </Card>
          <Card className="p-3">
            <Text size="2" weight="medium">Expiry</Text>
            <Text size="1" color="gray" className="mt-1 block">
              Optional number of days before the key expires. Leave blank for no expiry.
            </Text>
            <TextField.Root
              className="mt-3"
              type="number"
              min="1"
              value={expiresDays}
              onChange={(e) => setExpiresDays(e.target.value)}
              placeholder="90"
            />
          </Card>
        </div>

        <Card className="mt-3 p-3">
          <Text size="2" weight="medium">Scopes</Text>
          <Text size="1" color="gray" className="mt-1 block">
            Keep admin scopes off unless the client needs to manage approvals or other operator credentials.
          </Text>
          <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2">
            <ScopeToggleRow
              label="operator.read"
              description="Required for Mission Control connect and history access."
              checked={scopeToggles.read}
              onCheckedChange={(checked) => updateScopeToggle('read', checked)}
            />
            <ScopeToggleRow
              label="operator.write"
              description="Required for sending chats and starting runs."
              checked={scopeToggles.write}
              onCheckedChange={(checked) => updateScopeToggle('write', checked)}
            />
            <ScopeToggleRow
              label="operator.admin"
              description="Allows config changes and API key management."
              checked={scopeToggles.admin}
              onCheckedChange={(checked) => updateScopeToggle('admin', checked)}
            />
            <ScopeToggleRow
              label="operator.approvals"
              description="Allows approval queue operations."
              checked={scopeToggles.approvals}
              onCheckedChange={(checked) => updateScopeToggle('approvals', checked)}
            />
          </div>
        </Card>

        <Flex justify="end" className="mt-3">
          <Button onClick={() => void createKey()} disabled={busyAction === 'create'}>
            {busyAction === 'create' ? 'Creating...' : 'Create API key'}
          </Button>
        </Flex>
      </Card>

      {error ? (
        <Callout.Root color="red" size="1" variant="soft">
          <Callout.Text>{error}</Callout.Text>
        </Callout.Root>
      ) : null}
      {notice ? (
        <Callout.Root color="green" size="1" variant="soft">
          <Callout.Text>{notice}</Callout.Text>
        </Callout.Root>
      ) : null}

      <Card className="p-3">
        <Flex align="center" justify="between" gap="3" wrap="wrap">
          <div>
            <Text size="3" weight="bold">Existing keys</Text>
            <Text size="1" color="gray" className="mt-1 block">
              Active keys are shown first. Revoked keys remain listed for auditability.
            </Text>
          </div>
          <Button variant="soft" onClick={() => void loadKeys()} disabled={loading}>
            {loading ? 'Refreshing...' : 'Refresh'}
          </Button>
        </Flex>

        <div className="mt-3 space-y-3">
          {!loading && sortedKeys.length === 0 ? (
            <Text size="2" color="gray">No API keys yet.</Text>
          ) : null}
          {sortedKeys.map((item) => {
            const actionKeyRotate = `rotate:${item.id}`
            const actionKeyRevoke = `revoke:${item.id}`
            const isBusy = busyAction === actionKeyRotate || busyAction === actionKeyRevoke
            const revoked = Boolean(item.revoked_at)

            return (
              <Card key={item.id} className="p-3">
                <Flex align="start" justify="between" gap="3" wrap="wrap">
                  <div>
                    <Flex align="center" gap="2" wrap="wrap">
                      <Text size="2" weight="medium">{item.label}</Text>
                      <Badge variant="soft" color={revoked ? 'gray' : 'green'}>
                        {revoked ? 'revoked' : 'active'}
                      </Badge>
                      <Badge variant="soft" color="blue">{item.prefix}</Badge>
                    </Flex>
                    <Flex gap="2" wrap="wrap" className="mt-2">
                      {(item.scopes || []).map((scope) => (
                        <Badge key={`${item.id}-${scope}`} variant="soft" color="indigo">
                          {scope}
                        </Badge>
                      ))}
                    </Flex>
                    <Text size="1" color="gray" className="mt-2 block">
                      created {formatTimestamp(item.created_at)} | last used {formatTimestamp(item.last_used_at, 'never')}
                    </Text>
                    <Text size="1" color="gray" className="mt-1 block">
                      expires {formatTimestamp(item.expires_at, 'never')} | revoked {formatTimestamp(item.revoked_at, 'not revoked')}
                    </Text>
                  </div>
                  <Flex gap="2" wrap="wrap">
                    <Button
                      variant="soft"
                      disabled={revoked || isBusy}
                      onClick={() => void rotateKey(item)}
                    >
                      {busyAction === actionKeyRotate ? 'Rotating...' : 'Rotate'}
                    </Button>
                    <Button
                      variant="soft"
                      color="red"
                      disabled={revoked || isBusy}
                      onClick={() => void revokeKey(item)}
                    >
                      {busyAction === actionKeyRevoke ? 'Revoking...' : 'Revoke'}
                    </Button>
                  </Flex>
                </Flex>
              </Card>
            )
          })}
        </div>
      </Card>
    </div>
  )
}

function ScopeToggleRow({
  label,
  description,
  checked,
  onCheckedChange,
}: {
  label: string
  description: string
  checked: boolean
  onCheckedChange: (checked: boolean) => void
}) {
  return (
    <Card className="p-3">
      <Flex align="center" justify="between" gap="3">
        <div>
          <Text size="2" weight="medium">{label}</Text>
          <Text size="1" color="gray" className="mt-1 block">{description}</Text>
        </div>
        <Switch checked={checked} onCheckedChange={onCheckedChange} />
      </Flex>
    </Card>
  )
}

function formatTimestamp(value?: string | null, emptyLabel = 'Unknown'): string {
  if (!value) return emptyLabel
  const parsed = Date.parse(value)
  if (!Number.isFinite(parsed)) return value
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(parsed))
}
