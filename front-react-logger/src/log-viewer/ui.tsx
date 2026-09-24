import React from "react";
import { Pressable, Text, View, ActivityIndicator } from "react-native";
import { createButton } from "@gluestack-ui/core/button/creator";
import { OverlayProvider } from "@gluestack-ui/core/overlay/creator";
const Button = createButton({
  Root: Pressable,
  Text,
  Group: View,
  Spinner: ActivityIndicator,
  Icon: View,
});
export function GluestackUIProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  return <OverlayProvider>{children}</OverlayProvider>;
}
export function Action({
  children,
  onPress,
  disabled = false,
  primary = false,
  dark = false,
  label,
}: {
  children: React.ReactNode;
  onPress: () => void;
  disabled?: boolean;
  primary?: boolean;
  dark?: boolean;
  label?: string;
}) {
  return (
    <Button
      onPress={onPress}
      isDisabled={disabled}
      accessibilityLabel={label}
      style={{
        backgroundColor: primary ? "#087f72" : dark ? "#26343d" : "transparent",
        borderColor: primary ? "#087f72" : dark ? "#40505b" : "#d4dce3",
        borderWidth: 1,
        borderRadius: 6,
        paddingHorizontal: 14,
        paddingVertical: 9,
        opacity: disabled ? 0.5 : 1,
        alignSelf: "flex-start",
      }}
    >
      <Button.Text
        style={{
          color: primary || dark ? "#fff" : "#233441",
          fontSize: 14,
          fontWeight: "600",
          fontFamily: "system-ui",
        }}
      >
        {children}
      </Button.Text>
    </Button>
  );
}
