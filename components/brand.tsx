import type { ComponentProps } from 'react';

import { cn } from '@/lib/utils';

type BrandLogoProps = Omit<ComponentProps<'img'>, 'alt' | 'src'> & {
  surface?: 'dark' | 'light';
};

function BrandLogo({ className, surface = 'dark', ...props }: BrandLogoProps) {
  return (
    <img
      src={
        surface === 'dark' ? '/brand/logo-white.png' : '/brand/logo-black.png'
      }
      alt=""
      draggable={false}
      className={cn('select-none object-contain', className)}
      {...props}
    />
  );
}

type BrandLoaderProps = ComponentProps<'output'> & {
  label?: string;
};

function BrandLoader({
  className,
  label = 'Chargement',
  ...props
}: BrandLoaderProps) {
  return (
    <output
      data-slot="brand-loader"
      aria-label={label}
      className={cn(
        'relative inline-block size-5 shrink-0 overflow-hidden align-middle',
        className,
      )}
      {...props}
    >
      <img
        aria-hidden="true"
        className="pointer-events-none absolute left-1/2 top-1/2 h-full w-auto max-w-none -translate-x-1/2 -translate-y-1/2 scale-[1.35] mix-blend-screen motion-reduce:hidden"
        src="/brand/loading.webp"
        alt=""
      />
      <BrandLogo className="hidden size-full motion-reduce:block" />
    </output>
  );
}

export { BrandLoader, BrandLogo };
