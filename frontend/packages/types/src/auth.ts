export type Provider = 'gmail' | 'outlook' | 'imap' | 'pop3';

export type ArchiveStrategy = 'instant' | 'delayed' | 'manual';

export interface EmailAccount {
  id: string;
  provider: Provider;
  emailAddress: string;
  displayName?: string;
  archiveStrategy: ArchiveStrategy;
  syncDepth: string;
  labelPrefix: string;
  syncFrequency: number;
  isActive: boolean;
  lastSyncAt?: string;
  emailCount: number;
}

export interface OAuthCallbackParams {
  code: string;
  state: string;
}

export interface ImapConfig {
  email: string;
  password: string;
  imapServer: string;
  imapPort: number;
  smtpServer: string;
  smtpPort: number;
  encryption: 'ssl' | 'tls' | 'none';
}

/** Returned when an Outlook device-code sign-in starts. */
export interface DeviceCodeStart {
  flowId: string;
  /** The code the user types at `verificationUri`. */
  userCode: string;
  verificationUri: string;
  /** Seconds until the code expires. */
  expiresIn: number;
  /** Seconds to wait between polls. */
  interval: number;
}

export type DeviceCodeStatus = 'pending' | 'slow_down' | 'expired' | 'declined' | 'connected';

/** One poll of an Outlook device-code sign-in. */
export interface DeviceCodePoll {
  status: DeviceCodeStatus;
  /** Seconds to wait before polling again (pending, slow_down). */
  interval?: number;
  /** The connected account (connected). */
  account?: EmailAccount;
}
