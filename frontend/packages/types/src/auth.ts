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
