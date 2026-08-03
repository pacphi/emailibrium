// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ComposeEmail } from '../ComposeEmail';

vi.mock('@emailibrium/api', () => ({
  sendEmail: vi.fn(),
}));

afterEach(() => {
  cleanup();
});

function renderCompose(isOpen: boolean) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ComposeEmail
        isOpen={isOpen}
        onClose={vi.fn()}
        accounts={[{ id: 'acct-1', emailAddress: 'me@example.com', provider: 'gmail' }]}
      />
    </QueryClientProvider>,
  );
}

describe('ComposeEmail focus management', () => {
  it('renders nothing when closed', () => {
    const { container } = renderCompose(false);

    expect(container.querySelector('#compose-to')).toBeNull();
  });

  it('autofocuses the To field when opened, so keyboard focus stays inside the modal', () => {
    renderCompose(true);

    const toInput = document.getElementById('compose-to');
    expect(toInput).not.toBeNull();
    expect(document.activeElement).toBe(toInput);
  });
});
