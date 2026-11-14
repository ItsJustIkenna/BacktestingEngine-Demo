use diesel::prelude::*;

diesel::table! {
    exchanges (exchange_id) {
        exchange_id -> Int4,
        exchange -> Text,
    }
}

diesel::table! {
    securities (security_id) {
        security_id -> Int4,
        symbol -> Text,
    }
}

diesel::table! {
    order_books (order_book_id) {
        order_book_id -> Int4,
        symbol -> Text,
        exchange -> Text,
        security_id -> Int4,
        exchange_id -> Int4,
        buy_order_book_id -> Int4,
        sell_order_book_id -> Int4,
        total_volume -> Numeric,
    }
}

diesel::table! {
    historical_orders (id) {
        id -> Int8,
        timestamp -> Timestamptz,
        unique_id -> Text,
        order_id -> Text,
        event_type -> Text,
        side -> Text,
        price_level -> Numeric,
        quantity -> Numeric,
        remaining_quantity -> Nullable<Numeric>,
        fill_quantity -> Nullable<Numeric>,
        prev_price -> Nullable<Numeric>,
        prev_quantity -> Nullable<Numeric>,
        status -> Text,
        exchange -> Text,
        symbol -> Text,
        exchange_id -> Int4,
        security_id -> Int4,
    }
}

diesel::table! {
    historical_snapshots (id) {
        id -> Int8,
        timestamp -> Timestamptz,
        symbol -> Text,
        exchange -> Text,
        price_level -> Numeric,
        quantity -> Numeric,
        side -> Text,
        payload -> Jsonb,
    }
}

diesel::table! {
    trades (id) {
        id -> Int8,
        trade_id -> Text,
        symbol -> Text,
        exchange -> Text,
        security_id -> Int4,
        exchange_id -> Int4,
        side -> Text,
        price -> Numeric,
        quantity -> Numeric,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    open_buy_orders (id) {
        id -> Int8,
        timestamp -> Timestamptz,
        symbol -> Text,
        exchange -> Text,
        security_id -> Int4,
        exchange_id -> Int4,
        order_book_id -> Int4,
        unique_id -> Text,
        price_level -> Numeric,
        quantity -> Numeric,
    }
}

diesel::table! {
    open_sell_orders (id) {
        id -> Int8,
        timestamp -> Timestamptz,
        symbol -> Text,
        exchange -> Text,
        security_id -> Int4,
        exchange_id -> Int4,
        order_book_id -> Int4,
        unique_id -> Text,
        price_level -> Numeric,
        quantity -> Numeric,
    }
}

diesel::table! {
    wallet_balances (id) {
        id -> Int8,
        exchange -> Text,
        asset -> Text,
        wallet_type -> Text,
        wallet_id -> Text,
        balance -> Numeric,
        locked_balance -> Numeric,
        sequence -> Int8,
        timestamp -> Timestamptz,
    }
}

diesel::table! {
    l2_orderbook_snapshots (id) {
        id -> Int8,
        timestamp -> Timestamptz,
        symbol -> Text,
        exchange -> Text,
        security_id -> Int4,
        exchange_id -> Int4,
        bid_prices -> Array<Numeric>,
        bid_quantities -> Array<Numeric>,
        ask_prices -> Array<Numeric>,
        ask_quantities -> Array<Numeric>,
        best_bid_price -> Nullable<Numeric>,
        best_bid_quantity -> Nullable<Numeric>,
        best_ask_price -> Nullable<Numeric>,
        best_ask_quantity -> Nullable<Numeric>,
    }
}

diesel::table! {
    candles_1m (id) {
        id -> Int8,
        timestamp -> Timestamptz,
        symbol -> Text,
        exchange -> Text,
        open_price -> Numeric,
        high_price -> Numeric,
        low_price -> Numeric,
        close_price -> Numeric,
        volume -> Numeric,
        trade_count -> Int4,
    }
}

diesel::table! {
    candles_5m (id) {
        id -> Int8,
        timestamp -> Timestamptz,
        symbol -> Text,
        exchange -> Text,
        open_price -> Numeric,
        high_price -> Numeric,
        low_price -> Numeric,
        close_price -> Numeric,
        volume -> Numeric,
        trade_count -> Int4,
    }
}

diesel::table! {
    candles_15m (id) {
        id -> Int8,
        timestamp -> Timestamptz,
        symbol -> Text,
        exchange -> Text,
        open_price -> Numeric,
        high_price -> Numeric,
        low_price -> Numeric,
        close_price -> Numeric,
        volume -> Numeric,
        trade_count -> Int4,
    }
}

diesel::table! {
    candles_1h (id) {
        id -> Int8,
        timestamp -> Timestamptz,
        symbol -> Text,
        exchange -> Text,
        open_price -> Numeric,
        high_price -> Numeric,
        low_price -> Numeric,
        close_price -> Numeric,
        volume -> Numeric,
        trade_count -> Int4,
    }
}

diesel::table! {
    candles_1d (id) {
        id -> Int8,
        timestamp -> Timestamptz,
        symbol -> Text,
        exchange -> Text,
        open_price -> Numeric,
        high_price -> Numeric,
        low_price -> Numeric,
        close_price -> Numeric,
        volume -> Numeric,
        trade_count -> Int4,
    }
}

diesel::table! {
    strategies (id) {
        id -> Uuid,
        strategy_name -> Text,
        strategy_type -> Text,
        version -> Text,
        description -> Nullable<Text>,
        created_by -> Nullable<Text>,
        is_active -> Bool,
        base_configuration -> Nullable<Jsonb>,
        metadata -> Nullable<Jsonb>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    strategy_instances (id) {
        id -> Uuid,
        strategy_id -> Uuid,
        instance_name -> Nullable<Text>,
        description -> Nullable<Text>,
        parameters -> Jsonb,
        performance_summary -> Nullable<Jsonb>,
        risk_metrics -> Nullable<Jsonb>,
        is_template -> Bool,
        tags -> Nullable<Array<Nullable<Text>>>,
        created_by -> Nullable<Text>,
        optimization_run_id -> Nullable<Uuid>,
        optimization_score -> Nullable<Numeric>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    backtest_jobs (id) {
        id -> Uuid,
        job_id -> Text,
        symbol -> Text,
        exchange -> Text,
        strategy_type -> Text,
        optimization_method -> Text,
        population_size -> Nullable<Int4>,
        generations -> Nullable<Int4>,
        risk_aversion -> Numeric,
        inventory_target -> Numeric,
        order_size -> Numeric,
        initial_capital -> Numeric,
        commission_rate -> Numeric,
        start_date -> Nullable<Timestamptz>,
        end_date -> Nullable<Timestamptz>,
        params_json -> Jsonb,
        current_generation -> Nullable<Int4>,
        total_generations -> Nullable<Int4>,
        status -> Text,
        created_at -> Timestamptz,
        started_at -> Nullable<Timestamptz>,
        completed_at -> Nullable<Timestamptz>,
        last_heartbeat -> Nullable<Timestamptz>,
        progress -> Numeric,
        current_phase -> Nullable<Text>,
        phase_details -> Nullable<Jsonb>,
        error_message -> Nullable<Text>,
        result_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    backtest_results (id) {
        id -> Uuid,
        backtest_id -> Uuid,
        strategy_name -> Text,
        symbol -> Text,
        start_date -> Timestamptz,
        end_date -> Timestamptz,
        initial_capital -> Numeric,
        commission_rate -> Numeric,
        slippage_model_type -> Text,
        slippage_fixed_rate -> Nullable<Numeric>,
        slippage_sqrt_rate -> Nullable<Numeric>,
        slippage_linear_rate -> Nullable<Numeric>,
        temporary_impact -> Numeric,
        permanent_impact -> Numeric,
        participation_rate_limit -> Numeric,
        benchmark -> Nullable<Text>,
        rebalancing_frequency -> Text,
        point_in_time -> Bool,
        warmup_period_days -> Int4,
        total_return -> Numeric,
        annualized_return -> Numeric,
        volatility -> Numeric,
        sharpe_ratio -> Nullable<Numeric>,
        sortino_ratio -> Nullable<Numeric>,
        max_drawdown -> Numeric,
        calmar_ratio -> Nullable<Numeric>,
        win_rate -> Numeric,
        profit_factor -> Numeric,
        avg_trade_return -> Numeric,
        total_trades -> Int4,
        best_trade -> Nullable<Numeric>,
        worst_trade -> Nullable<Numeric>,
        avg_time_in_trade -> Nullable<Numeric>,
        value_at_risk_95 -> Nullable<Numeric>,
        expected_shortfall_95 -> Nullable<Numeric>,
        beta -> Nullable<Numeric>,
        correlation_with_benchmark -> Nullable<Numeric>,
        tracking_error -> Nullable<Numeric>,
        information_ratio -> Nullable<Numeric>,
        jensen_alpha -> Nullable<Numeric>,
        max_drawdown_duration_days -> Nullable<Int4>,
        current_drawdown -> Numeric,
        avg_drawdown -> Nullable<Numeric>,
        benchmark_return -> Nullable<Numeric>,
        excess_return -> Nullable<Numeric>,
        outperformance_periods -> Nullable<Int4>,
        underperformance_periods -> Nullable<Int4>,
        total_orders -> Int4,
        filled_orders -> Int4,
        cancelled_orders -> Int4,
        avg_slippage -> Numeric,
        total_commission_paid -> Numeric,
        avg_fill_time_seconds -> Nullable<Numeric>,
        strategy_metrics -> Nullable<Jsonb>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        strategy_instance_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    backtest_reports (id) {
        id -> Uuid,
        backtest_result_id -> Uuid,
        report_id -> Text,
        report_name -> Text,
        strategy_name -> Text,
        symbol -> Text,
        timeframe -> Text,
        start_date -> Date,
        end_date -> Date,
        initial_capital -> Numeric,
        generated_at -> Timestamptz,
        generated_by -> Nullable<Text>,
        generation_source -> Text,
        backtest_duration_seconds -> Nullable<Numeric>,
        data_points -> Nullable<Int4>,
        include_trades -> Bool,
        include_charts -> Bool,
        export_formats -> Array<Text>,
        custom_css -> Nullable<Text>,
        template_version -> Nullable<Text>,
        file_paths -> Jsonb,
        file_sizes -> Nullable<Jsonb>,
        storage_location -> Text,
        performance_summary -> Jsonb,
        risk_summary -> Jsonb,
        trade_summary -> Jsonb,
        status -> Text,
        error_message -> Nullable<Text>,
        tags -> Nullable<Array<Text>>,
        notes -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        accessed_at -> Nullable<Timestamptz>,
        access_count -> Int4,
    }
}

diesel::table! {
    backtest_report_access_log (id) {
        id -> Uuid,
        report_id -> Uuid,
        accessed_by -> Nullable<Text>,
        access_method -> Text,
        format_requested -> Nullable<Text>,
        user_agent -> Nullable<Text>,
        ip_address -> Nullable<Text>,
        response_time_ms -> Nullable<Int4>,
        success -> Bool,
        error_message -> Nullable<Text>,
        accessed_at -> Timestamptz,
    }
}
