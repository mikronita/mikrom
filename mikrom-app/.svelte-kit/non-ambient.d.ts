
// this file is generated — do not edit it


declare module "svelte/elements" {
	export interface HTMLAttributes<T> {
		'data-sveltekit-keepfocus'?: true | '' | 'off' | undefined | null;
		'data-sveltekit-noscroll'?: true | '' | 'off' | undefined | null;
		'data-sveltekit-preload-code'?:
			| true
			| ''
			| 'eager'
			| 'viewport'
			| 'hover'
			| 'tap'
			| 'off'
			| undefined
			| null;
		'data-sveltekit-preload-data'?: true | '' | 'hover' | 'tap' | 'off' | undefined | null;
		'data-sveltekit-reload'?: true | '' | 'off' | undefined | null;
		'data-sveltekit-replacestate'?: true | '' | 'off' | undefined | null;
	}
}

export {};


declare module "$app/types" {
	type MatcherParam<M> = M extends (param : string) => param is (infer U extends string) ? U : string;

	export interface AppTypes {
		RouteId(): "/" | "/api" | "/api/v1" | "/api/v1/[...path]" | "/apps" | "/apps/[appName]" | "/auth" | "/auth/login" | "/auth/register" | "/networking" | "/settings" | "/storage";
		RouteParams(): {
			"/api/v1/[...path]": { path: string };
			"/apps/[appName]": { appName: string }
		};
		LayoutParams(): {
			"/": { path?: string; appName?: string };
			"/api": { path?: string };
			"/api/v1": { path?: string };
			"/api/v1/[...path]": { path: string };
			"/apps": { appName?: string };
			"/apps/[appName]": { appName: string };
			"/auth": Record<string, never>;
			"/auth/login": Record<string, never>;
			"/auth/register": Record<string, never>;
			"/networking": Record<string, never>;
			"/settings": Record<string, never>;
			"/storage": Record<string, never>
		};
		Pathname(): "/" | `/api/v1/${string}` & {} | "/apps" | `/apps/${string}` & {} | "/auth/login" | "/auth/register" | "/networking" | "/settings" | "/storage";
		ResolvedPathname(): `${"" | `/${string}`}${ReturnType<AppTypes['Pathname']>}`;
		Asset(): "/icon.svg" | string & {};
	}
}