<script lang="ts">
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import frameIcon from '$lib/assets/icons/frame.svg?raw';

	onMount(() => {
		getCurrentWindow().show();
	});
</script>

<div
	class="flex h-screen w-screen cursor-wait flex-col items-center justify-center overflow-hidden rounded-2xl bg-transparent select-none"
	data-tauri-drag-region
>
	<div class="relative flex items-center justify-center">
		<div
			class="flex size-32 items-center justify-center text-foreground [&>svg]:size-full [&>svg]:fill-current"
		>
			<!-- eslint-disable-next-line svelte/no-at-html-tags -->
			{@html frameIcon}
		</div>
	</div>
</div>
